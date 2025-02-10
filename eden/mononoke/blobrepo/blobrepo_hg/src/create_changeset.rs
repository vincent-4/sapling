/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;
use std::sync::Mutex;

use ::manifest::Entry;
use anyhow::anyhow;
use anyhow::format_err;
use anyhow::Context;
use anyhow::Error;
use anyhow::Result;
use blobstore::Loadable;
use bonsai_hg_mapping::BonsaiHgMappingArc;
use bonsai_hg_mapping::BonsaiHgMappingEntry;
use bonsai_hg_mapping::BonsaiHgMappingRef;
use cloned::cloned;
use commit_graph::CommitGraphWriterArc;
use context::CoreContext;
use futures::channel::oneshot;
use futures::future;
use futures::future::BoxFuture;
use futures::future::FutureExt;
use futures::future::TryFutureExt;
use futures::stream::BoxStream;
use futures_ext::FbTryFutureExt;
use futures_stats::TimedTryFutureExt;
use mercurial_types::blobs::ChangesetMetadata;
use mercurial_types::blobs::HgBlobChangeset;
use mercurial_types::HgFileNodeId;
use mercurial_types::HgManifestId;
use mercurial_types::HgNodeHash;
use mercurial_types::RepoPath;
use mononoke_macros::mononoke;
use mononoke_types::BlobstoreValue;
use mononoke_types::BonsaiChangeset;
use mononoke_types::NonRootMPath;
use repo_blobstore::RepoBlobstoreArc;
use repo_blobstore::RepoBlobstoreRef;
use scuba_ext::MononokeScubaSampleBuilder;
use stats::prelude::*;
use uuid::Uuid;
use wireproto_handler::BackupSourceRepo;

use crate::bonsai_generation::create_bonsai_changeset_object;
use crate::bonsai_generation::save_bonsai_changeset_object;
use crate::repo_commit::*;
use crate::ErrorKind;

define_stats! {
    prefix = "mononoke.blobrepo";
    create_changeset: timeseries(Rate, Sum),
    create_changeset_compute_cf: timeseries("create_changeset.compute_changed_files"; Rate, Sum),
    create_changeset_expected_cf: timeseries("create_changeset.expected_changed_files"; Rate, Sum),
    create_changeset_cf_count: timeseries("create_changeset.changed_files_count"; Average, Sum),
}

async fn verify_bonsai_changeset_with_origin(
    ctx: &CoreContext,
    bcs: BonsaiChangeset,
    cs: &HgBlobChangeset,
    origin_repo: &BackupSourceRepo,
) -> Result<BonsaiChangeset, Error> {
    // There are some non-canonical bonsai changesets in the prod repos.
    // To make the blobimported backup repos exactly the same, we will
    // fetch bonsai from the prod in case of mismatch
    let origin_bonsai_id = origin_repo
        .bonsai_hg_mapping()
        .get_bonsai_from_hg(ctx, cs.get_changeset_id())
        .await?;
    match origin_bonsai_id {
        Some(id) if id != bcs.get_changeset_id() => {
            id.load(ctx, origin_repo.repo_blobstore())
                .map_err(|e| anyhow!(e))
                .await
        }
        _ => Ok(bcs),
    }
}

pub struct CreateChangeset {
    /// This should always be provided, keeping it an Option for tests
    pub expected_nodeid: Option<HgNodeHash>,
    pub expected_files: Option<Vec<NonRootMPath>>,
    pub p1: Option<ChangesetHandle>,
    pub p2: Option<ChangesetHandle>,
    // root_manifest can be None f.e. when commit removes all the content of the repo
    pub root_manifest: BoxFuture<'static, Result<Option<(HgManifestId, RepoPath)>>>,
    pub sub_entries: BoxStream<'static, Result<(Entry<HgManifestId, HgFileNodeId>, RepoPath)>>,
    pub cs_metadata: ChangesetMetadata,
    pub verify_origin_repo: Option<BackupSourceRepo>,
    /// If set to true, don't update Changesets or BonsaiHgMapping, which should be done
    /// manually after this call. Effectively, the commit will be in the blobstore, but
    /// unreachable.
    pub upload_to_blobstore_only: bool,
}

impl CreateChangeset {
    pub fn create(
        self,
        ctx: CoreContext,
        repo: &(impl RepoBlobstoreArc + CommitGraphWriterArc + BonsaiHgMappingArc + Send + Sync),
        bonsai: Option<BonsaiChangeset>,
        mut scuba_logger: MononokeScubaSampleBuilder,
    ) -> ChangesetHandle {
        STATS::create_changeset.add_value(1);
        // This is used for logging, so that we can tie up all our pieces without knowing about
        // the final commit hash
        let uuid = Uuid::new_v4();
        scuba_logger.add("changeset_uuid", format!("{}", uuid));

        let entry_processor =
            UploadEntries::new(repo.repo_blobstore().clone(), scuba_logger.clone());
        let (signal_parent_ready, can_be_parent) = oneshot::channel();
        let signal_parent_ready = Arc::new(Mutex::new(Some(signal_parent_ready)));
        let expected_nodeid = self.expected_nodeid;

        let upload_entries = {
            cloned!(ctx, entry_processor);
            let root_manifest = self.root_manifest;
            let sub_entries = self.sub_entries;
            async move {
                process_entries(&ctx, &entry_processor, root_manifest, sub_entries)
                    .await
                    .context("While processing entries")
            }
        };

        let parents_complete = extract_parents_complete(&self.p1, &self.p2)
            .try_timed()
            .map({
                let mut scuba_logger = scuba_logger.clone();
                move |result| match result {
                    Err(err) => Err(err.context("While waiting for parents to complete")),
                    Ok((stats, result)) => {
                        scuba_logger
                            .add_future_stats(&stats)
                            .log_with_msg("Parents completed", None);
                        Ok(result)
                    }
                }
            });
        let parents_data = handle_parents(scuba_logger.clone(), self.p1, self.p2)
            .map_err(|err| err.context("While waiting for parents to upload data"));

        let changeset = {
            cloned!(ctx, signal_parent_ready, mut scuba_logger);
            let expected_files = self.expected_files;
            let cs_metadata = self.cs_metadata;
            let blobstore = repo.repo_blobstore_arc();

            async move {
                let (root_mf_id, (parents, parent_manifest_hashes, bonsai_parents)) =
                    future::try_join(upload_entries, parents_data).await?;
                let files = if let Some(expected_files) = expected_files {
                    STATS::create_changeset_expected_cf.add_value(1);
                    // We are trusting the callee to provide a list of changed files, used
                    // by the import job
                    expected_files
                } else {
                    STATS::create_changeset_compute_cf.add_value(1);
                    compute_changed_files(
                        ctx.clone(),
                        blobstore.clone(),
                        root_mf_id,
                        parent_manifest_hashes.first().cloned(),
                        parent_manifest_hashes.get(1).cloned(),
                    )
                    .await?
                };

                STATS::create_changeset_cf_count.add_value(files.len() as i64);
                let hg_cs = make_new_changeset(parents, root_mf_id, cs_metadata, files)?;

                let (bonsai_cs, bcs_fut) = match bonsai {
                    Some(bonsai_cs) => (bonsai_cs, async move { Ok(()) }.boxed()),
                    None => {
                        let bonsai_cs = create_bonsai_changeset_object(
                            &ctx,
                            hg_cs.clone(),
                            parent_manifest_hashes.clone(),
                            bonsai_parents,
                            &blobstore,
                        )
                        .await?;
                        let bonsai_cs = if let Some(origin_repo) = self.verify_origin_repo.as_ref()
                        {
                            verify_bonsai_changeset_with_origin(
                                &ctx,
                                bonsai_cs,
                                &hg_cs,
                                origin_repo,
                            )
                            .await?
                        } else {
                            bonsai_cs
                        };

                        (
                            bonsai_cs.clone(),
                            save_bonsai_changeset_object(&ctx, &blobstore, bonsai_cs).boxed(),
                        )
                    }
                };
                let bonsai_blob = bonsai_cs.clone().into_blob();
                let bcs_id = bonsai_blob.id().clone();
                let cs_id = hg_cs.get_changeset_id().into_nodehash();
                let manifest_id = hg_cs.manifestid();

                if let Some(expected_nodeid) = expected_nodeid {
                    if cs_id != expected_nodeid {
                        return Err(ErrorKind::InconsistentChangesetHash(
                            expected_nodeid,
                            cs_id,
                            hg_cs,
                        )
                        .into());
                    }
                }

                scuba_logger
                    .add("changeset_id", format!("{}", cs_id))
                    .log_with_msg("Changeset uuid to hash mapping", None);
                // NOTE(luk): an attempt was made in D8187210 to split the
                // upload_entries signal into upload_entries and
                // processed_entries and to signal_parent_ready after
                // upload_entries, so that one doesn't need to wait for the
                // entries to be processed. There were no performance gains
                // from that experiment
                //
                // We deliberately eat this error - this is only so that
                // another changeset can start verifying data in the blob
                // store while we verify this one
                let _ = signal_parent_ready
                    .lock()
                    .expect("poisoned lock")
                    .take()
                    .expect("signal_parent_ready cannot be taken yet")
                    .send(Ok((bcs_id, cs_id, manifest_id)));

                futures::try_join!(
                    bcs_fut,
                    hg_cs.save(&ctx, &blobstore),
                    entry_processor
                        .finalize(&ctx, root_mf_id, parent_manifest_hashes)
                        .map_err(|err| err.context("While finalizing processing")),
                )?;

                Ok::<_, Error>((hg_cs, bonsai_cs))
            }
        }
        .try_timed()
        .map({
            cloned!(mut scuba_logger);
            move |result| {
                match result {
                    Ok((stats, result)) => {
                        scuba_logger
                            .add_future_stats(&stats)
                            .log_with_msg("Changeset created", None);
                        Ok(result)
                    }
                    Err(err) => {
                        let err =
                            err.context("While creating and verifying Changeset for blobstore");
                        let trigger = signal_parent_ready.lock().expect("poisoned lock").take();
                        if let Some(trigger) = trigger {
                            // Ignore errors if the receiving end has gone away.
                            let e = format_err!("signal_parent_ready failed: {:?}", err);
                            let _ = trigger.send(Err(e));
                        }
                        Err(err)
                    }
                }
            }
        });

        let commit_graph_writer = repo.commit_graph_writer_arc();
        let bonsai_hg_mapping = repo.bonsai_hg_mapping_arc();
        let changeset_complete_fut = async move {
            let ((hg_cs, bonsai_cs), _) = future::try_join(changeset, parents_complete).await?;

            if !self.upload_to_blobstore_only {
                // update changeset mapping
                commit_graph_writer
                    .add(
                        &ctx,
                        bonsai_cs.get_changeset_id(),
                        bonsai_cs.parents().collect(),
                    )
                    .await
                    .context("While inserting into changeset table")?;

                // update bonsai mapping
                let bcs_id = bonsai_cs.get_changeset_id();
                let bonsai_hg_entry = BonsaiHgMappingEntry {
                    hg_cs_id: hg_cs.get_changeset_id(),
                    bcs_id,
                };
                bonsai_hg_mapping
                    .add(&ctx, bonsai_hg_entry)
                    .await
                    .context("While inserting mapping")?;
            }

            Ok::<_, Error>((bonsai_cs, hg_cs))
        }
        .try_timed()
        .map({
            cloned!(mut scuba_logger);
            move |result| match result {
                Ok((stats, result)) => {
                    scuba_logger
                        .add_future_stats(&stats)
                        .log_with_msg("CreateChangeset Finished", None);
                    Ok(result)
                }
                Err(err) => Err(err.context(format!(
                    "While creating Changeset {:?}, uuid: {}",
                    expected_nodeid, uuid
                ))),
            }
        });

        let can_be_parent = can_be_parent
            .map(|r| match r {
                Ok(res) => res,
                Err(e) => Err(format_err!("can_be_parent: {:?}", e)),
            })
            .boxed()
            .try_shared();

        let completion_future = mononoke::spawn_task(changeset_complete_fut)
            .map(|result| result?)
            .boxed()
            .try_shared();

        ChangesetHandle::new_pending(can_be_parent, completion_future)
    }
}
