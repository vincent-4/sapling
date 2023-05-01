/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashSet;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use std::time::SystemTime;

use anyhow::anyhow;
use anyhow::Result;
use configmodel::Config;
use configmodel::ConfigExt;
use io::IO;
use manifest_tree::ReadTreeManifest;
use parking_lot::Mutex;
use pathmatcher::AlwaysMatcher;
use pathmatcher::DifferenceMatcher;
use pathmatcher::ExactMatcher;
use pathmatcher::Matcher;
use pathmatcher::NeverMatcher;
use progress_model::ProgressBar;
use repolock::RepoLocker;
use serde::Deserialize;
use serde::Serialize;
use treestate::filestate::StateFlags;
use treestate::treestate::TreeState;
use types::path::ParseError;
use types::RepoPathBuf;
use vfs::VFS;
use watchman_client::prelude::*;

use super::treestate::clear_needs_check;
use super::treestate::mark_needs_check;
use super::treestate::set_clock;
use crate::filechangedetector::ArcReadFileContents;
use crate::filechangedetector::FileChangeDetector;
use crate::filechangedetector::FileChangeDetectorTrait;
use crate::filechangedetector::ResolvedFileChangeResult;
use crate::filesystem::ChangeType;
use crate::filesystem::PendingChangeResult;
use crate::filesystem::PendingChanges;
use crate::metadata;
use crate::metadata::Metadata;
use crate::util::walk_treestate;
use crate::watchmanfs::treestate::get_clock;
use crate::watchmanfs::treestate::list_needs_check;
use crate::watchmanfs::treestate::maybe_flush_treestate;
use crate::workingcopy::WorkingCopy;

type ArcReadTreeManifest = Arc<dyn ReadTreeManifest + Send + Sync>;

pub struct WatchmanFileSystem {
    vfs: VFS,
    treestate: Arc<Mutex<TreeState>>,
    tree_resolver: ArcReadTreeManifest,
    store: ArcReadFileContents,
    locker: Arc<RepoLocker>,
}

struct WatchmanConfig {
    clock: Option<Clock>,
    sync_timeout: std::time::Duration,
}

query_result_type! {
    pub struct StatusQuery {
        name: BytesNameField,
        mode: ModeAndPermissionsField,
        size: SizeField,
        mtime: MTimeField,
        exists: ExistsField,
    }
}

#[derive(Deserialize, Debug)]
struct DebugRootStatusResponse {
    pub root_status: Option<RootStatus>,
}

#[derive(Deserialize, Debug)]
struct RootStatus {
    pub recrawl_info: Option<RecrawlInfo>,
}

#[derive(Deserialize, Debug)]
pub struct RecrawlInfo {
    pub stats: Option<u64>,
}

#[derive(Serialize, Clone, Debug)]
pub struct DebugRootStatusRequest(pub &'static str, pub PathBuf);

impl WatchmanFileSystem {
    pub fn new(
        vfs: VFS,
        treestate: Arc<Mutex<TreeState>>,
        tree_resolver: ArcReadTreeManifest,
        store: ArcReadFileContents,
        locker: Arc<RepoLocker>,
    ) -> Result<Self> {
        Ok(WatchmanFileSystem {
            vfs,
            treestate,
            tree_resolver,
            store,
            locker,
        })
    }

    #[tracing::instrument(skip_all, err)]
    async fn query_files(&self, config: WatchmanConfig) -> Result<QueryResult<StatusQuery>> {
        let start = std::time::Instant::now();

        // This starts watchman if it isn't already started.
        let client = Connector::new().connect().await?;

        // This blocks until the recrawl (if required) is done. Progress is
        // shown by the crawl_progress task.
        let resolved = client
            .resolve_root(CanonicalPath::canonicalize(self.vfs.root())?)
            .await?;

        let ident = identity::must_sniff_dir(self.vfs.root())?;
        let excludes = Expr::Any(vec![Expr::DirName(DirNameTerm {
            path: PathBuf::from(ident.dot_dir()),
            depth: None,
        })]);

        // The crawl is done - display a generic "we're querying" spinner.
        let _bar = ProgressBar::register_new("querying watchman", 0, "");

        let result = client
            .query::<StatusQuery>(
                &resolved,
                QueryRequestCommon {
                    since: config.clock,
                    expression: Some(Expr::Not(Box::new(excludes))),
                    sync_timeout: config.sync_timeout.into(),
                    ..Default::default()
                },
            )
            .await?;

        tracing::trace!(target: "measuredtimes", watchmanquery_time=start.elapsed().as_millis());

        Ok(result)
    }
}

async fn crawl_progress(root: PathBuf, approx_file_count: u64) -> Result<()> {
    let client = {
        let _bar = ProgressBar::register_new("connecting watchman", 0, "");

        // If watchman just started (and we issued "watch-project" from
        // query_files), this connect gets stuck indefinitely. Work around by
        // timing out and retrying until we get through.
        loop {
            match tokio::time::timeout(Duration::from_secs(1), Connector::new().connect()).await {
                Ok(client) => break client?,
                Err(_) => {}
            };

            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    };

    let mut bar = None;

    let req = DebugRootStatusRequest(
        "debug-root-status",
        CanonicalPath::canonicalize(root)?.into_path_buf(),
    );

    loop {
        let response: DebugRootStatusResponse = client.generic_request(req.clone()).await?;

        if let Some(RootStatus {
            recrawl_info: Some(RecrawlInfo { stats: Some(stats) }),
        }) = response.root_status
        {
            bar.get_or_insert_with(|| {
                ProgressBar::register_new("crawling", approx_file_count, "files (approx)")
            })
            .set_position(stats);
        } else if bar.is_some() {
            return Ok(());
        }

        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

impl PendingChanges for WatchmanFileSystem {
    #[tracing::instrument(skip_all)]
    fn pending_changes(
        &self,
        matcher: Arc<dyn Matcher + Send + Sync + 'static>,
        mut ignore_matcher: Arc<dyn Matcher + Send + Sync + 'static>,
        last_write: SystemTime,
        config: &dyn Config,
        io: &IO,
    ) -> Result<Box<dyn Iterator<Item = Result<PendingChangeResult>>>> {
        let ts = &mut *self.treestate.lock();

        let ts_metadata = ts.metadata()?;
        let mut prev_clock = get_clock(&ts_metadata)?;

        let track_ignored = config.get_or_default::<bool>("fsmonitor", "track-ignore-files")?;
        let ts_track_ignored = ts_metadata.get("track-ignored").map(|v| v.as_ref()) == Some("1");
        if track_ignored != ts_track_ignored {
            // If track-ignore-files has changed, trigger a migration by
            // unsetting the clock. Watchman will do a full crawl and report
            // fresh instance.
            prev_clock = None;

            // Store new value of track ignored so we don't migrate again.
            let md_value = if track_ignored {
                "1".to_string()
            } else {
                "0".to_string()
            };
            tracing::info!(track_ignored = md_value, "migrating track-ignored");
            ts.update_metadata(&[("track-ignored".to_string(), Some(md_value))])?;
        }

        let progress_handle = async_runtime::spawn(crawl_progress(
            self.vfs.root().to_path_buf(),
            ts.len() as u64,
        ));

        let result = async_runtime::block_on(self.query_files(WatchmanConfig {
            clock: prev_clock.clone(),
            sync_timeout:
                config.get_or::<Duration>("fsmonitor", "timeout", || Duration::from_secs(10))?,
        }))?;

        progress_handle.abort();

        tracing::debug!(
            target: "watchman_info",
            watchmanfreshinstances= if result.is_fresh_instance { 1 } else { 0 },
            watchmanfilecount=result.files.as_ref().map_or(0, |f| f.len()),
        );

        let should_warn = config.get_or_default("fsmonitor", "warn-fresh-instance")?;
        if result.is_fresh_instance && should_warn {
            let _ = warn_about_fresh_instance(
                io,
                parse_watchman_pid(prev_clock.as_ref()),
                parse_watchman_pid(Some(&result.clock)),
            );
        }

        let file_change_threshold =
            config.get_or("fsmonitor", "watchman-changed-file-threshold", || 200)?;
        let should_update_clock = result.is_fresh_instance
            || result
                .files
                .as_ref()
                .map_or(false, |f| f.len() > file_change_threshold);

        let manifests = WorkingCopy::current_manifests(ts, &self.tree_resolver)?;

        let mut wm_errors: Vec<ParseError> = Vec::new();
        let use_watchman_metadata =
            config.get_or::<bool>("workingcopy", "use-watchman-metadata", || true)?;
        let wm_needs_check: Vec<metadata::File> = result
            .files
            .unwrap_or_default()
            .into_iter()
            .filter_map(
                |file| match RepoPathBuf::from_utf8(file.name.into_inner().into_bytes()) {
                    Ok(path) => {
                        tracing::trace!(
                            ?path,
                            mode = *file.mode,
                            size = *file.size,
                            mtime = *file.mtime,
                            exists = *file.exists,
                            "watchman file"
                        );

                        let fs_meta = if *file.exists {
                            if use_watchman_metadata {
                                Some(Some(Metadata::from_stat(
                                    file.mode.into_inner() as u32,
                                    file.size.into_inner(),
                                    file.mtime.into_inner(),
                                )))
                            } else {
                                None
                            }
                        } else {
                            // If watchman says the file doesn't exist, indicate
                            // that via the metadata being None. This is
                            // important when a file moves behind a symlink;
                            // Watchman will report it as deleted, but a naive
                            // lstat() call would show the file to still exist.
                            Some(None)
                        };

                        Some(metadata::File {
                            path,
                            fs_meta,
                            ts_state: None,
                        })
                    }
                    Err(err) => {
                        wm_errors.push(err);
                        None
                    }
                },
            )
            .collect();

        if track_ignored {
            // If we want to track ignored files, say that nothing is ignored.
            // Note that the "full" matcher will still skip ignored files.
            ignore_matcher = Arc::new(NeverMatcher::new());
        }

        let detector = FileChangeDetector::new(
            self.vfs.clone(),
            last_write.try_into()?,
            manifests[0].clone(),
            self.store.clone(),
            config.get_opt("workingcopy", "worker-count")?,
        );
        let mut pending_changes = detect_changes(
            matcher,
            ignore_matcher,
            detector,
            ts,
            wm_needs_check,
            result.is_fresh_instance,
            self.vfs.case_sensitive(),
        )?;

        // Add back path errors into the pending changes. The caller
        // of pending_changes must choose how to handle these.
        pending_changes
            .pending_changes
            .extend(wm_errors.into_iter().map(|e| Err(anyhow!(e))));

        let did_something = pending_changes.update_treestate(ts)?;
        if did_something || should_update_clock {
            // If we had something to update in the treestate, make sure clock is updated as well.
            set_clock(ts, result.clock)?;
        }

        maybe_flush_treestate(self.vfs.root(), ts, &self.locker)?;

        Ok(Box::new(pending_changes.into_iter()))
    }
}

fn warn_about_fresh_instance(io: &IO, old_pid: Option<u32>, new_pid: Option<u32>) -> Result<()> {
    let mut output = io.error();
    match (old_pid, new_pid) {
        (Some(old_pid), Some(new_pid)) if old_pid != new_pid => {
            writeln!(
                &mut output,
                "warning: watchman has recently restarted (old pid {}, new pid {}) - operation will be slower than usual",
                old_pid, new_pid
            )?;
        }
        (None, Some(new_pid)) => {
            writeln!(
                &mut output,
                "warning: watchman has recently started (pid {}) - operation will be slower than usual",
                new_pid
            )?;
        }
        _ => {
            writeln!(
                &mut output,
                "warning: watchman failed to catch up with file change events and requires a full scan - operation will be slower than usual"
            )?;
        }
    }

    Ok(())
}

// Given the existing treestate and files watchman says to check,
// figure out all the files that may have changed and check them for
// changes. Also track paths we need to mark or unmark as NEED_CHECK
// in the treestate.
pub(crate) fn detect_changes(
    matcher: Arc<dyn Matcher + Send + Sync + 'static>,
    ignore_matcher: Arc<dyn Matcher + Send + Sync + 'static>,
    mut file_change_detector: impl FileChangeDetectorTrait + 'static,
    ts: &mut TreeState,
    wm_need_check: Vec<metadata::File>,
    wm_fresh_instance: bool,
    fs_case_sensitive: bool,
) -> Result<WatchmanPendingChanges> {
    let (ts_need_check, ts_errors) = list_needs_check(ts, matcher)?;

    // NB: ts_need_check is filtered by the matcher, so it does not
    // necessarily contain all NEED_CHECK entries in the treestate.
    let ts_need_check: HashSet<_> = ts_need_check.into_iter().collect();

    let mut pending_changes: Vec<Result<PendingChangeResult>> =
        ts_errors.into_iter().map(|e| Err(anyhow!(e))).collect();
    let mut needs_clear = Vec::new();
    let mut needs_mark = Vec::new();

    tracing::debug!(
        watchman_needs_check = wm_need_check.len(),
        treestate_needs_check = ts_need_check.len(),
    );

    let total_needs_check = ts_need_check.len()
        + wm_need_check
            .iter()
            .filter(|p| !ts_need_check.contains(&p.path))
            .count();

    // This is to set "total" for progress bar.
    file_change_detector.total_work_hint(total_needs_check as u64);

    let wm_seen: HashSet<RepoPathBuf> = wm_need_check.iter().map(|f| f.path.clone()).collect();

    for ts_needs_check in ts_need_check.iter() {
        // Prefer to kick off file check using watchman data since that already
        // includes disk metadata.
        if wm_seen.contains(ts_needs_check) {
            continue;
        }

        // We don't need the ignore check since ts_need_check was filtered by
        // the full matcher, which incorporates the ignore matcher.
        file_change_detector.submit(metadata::File {
            path: ts_needs_check.clone(),
            ts_state: ts.normalized_get(ts_needs_check)?,
            fs_meta: None,
        })
    }

    for mut wm_needs_check in wm_need_check {
        let state = ts.normalized_get(&wm_needs_check.path)?;

        let is_tracked = match &state {
            Some(state) => state
                .state
                .intersects(StateFlags::EXIST_P1 | StateFlags::EXIST_P2 | StateFlags::EXIST_NEXT),
            None => false,
        };
        // Skip ignored files to reduce work. We short circuit with an
        // "untracked" check to minimize use of the GitignoreMatcher.
        if !is_tracked && ignore_matcher.matches_file(&wm_needs_check.path)? {
            continue;
        }

        wm_needs_check.ts_state = state;

        file_change_detector.submit(wm_needs_check);
    }

    for result in file_change_detector {
        match result {
            Ok(ResolvedFileChangeResult::Yes(change)) => {
                let path = change.get_path();
                if !ts_need_check.contains(path) {
                    needs_mark.push(path.clone());
                }
                pending_changes.push(Ok(PendingChangeResult::File(change)));
            }
            Ok(ResolvedFileChangeResult::No(path)) => {
                if ts_need_check.contains(&path) {
                    needs_clear.push(path);
                }
            }
            Err(e) => pending_changes.push(Err(e)),
        }
    }

    if wm_fresh_instance {
        let was_deleted_matcher = Arc::new(DifferenceMatcher::new(
            AlwaysMatcher::new(),
            ExactMatcher::new(wm_seen.iter(), fs_case_sensitive),
        ));

        // On fresh instance, watchman returns all files present on
        // disk. We need to catch the case where a tracked file has been
        // deleted while watchman wasn't running. To do that, report a
        // pending "delete" change for all EXIST_NEXT files that were
        // _not_ in the list we got from watchman.
        walk_treestate(
            ts,
            was_deleted_matcher.clone(),
            StateFlags::EXIST_NEXT,
            StateFlags::NEED_CHECK,
            |path, _state| {
                needs_mark.push(path.clone());
                pending_changes.push(Ok(PendingChangeResult::File(ChangeType::Deleted(path))));
                Ok(())
            },
        )?;

        // Clear out ignored/untracked files that have been deleted.
        walk_treestate(
            ts,
            was_deleted_matcher,
            StateFlags::NEED_CHECK,
            StateFlags::EXIST_NEXT | StateFlags::EXIST_P1 | StateFlags::EXIST_P2,
            |path, _state| {
                needs_clear.push(path);
                Ok(())
            },
        )?;
    }

    Ok(WatchmanPendingChanges {
        pending_changes,
        needs_clear,
        needs_mark,
    })
}

pub struct WatchmanPendingChanges {
    pending_changes: Vec<Result<PendingChangeResult>>,
    needs_clear: Vec<RepoPathBuf>,
    needs_mark: Vec<RepoPathBuf>,
}

impl WatchmanPendingChanges {
    #[tracing::instrument(skip_all)]
    pub fn update_treestate(&mut self, ts: &mut TreeState) -> Result<bool> {
        let bar = ProgressBar::register_new(
            "recording files",
            (self.needs_clear.len() + self.needs_mark.len()) as u64,
            "entries",
        );

        let mut wrote = false;
        for path in self.needs_clear.iter() {
            match clear_needs_check(ts, path) {
                Ok(v) => wrote |= v,
                Err(e) =>
                // We can still build a valid result if we fail to clear the
                // needs check flag. Propagate the error to the caller but allow
                // the persist to continue.
                {
                    self.pending_changes.push(Err(e))
                }
            }

            bar.increase_position(1);
        }

        for path in self.needs_mark.iter() {
            wrote |= mark_needs_check(ts, path)?;
            bar.increase_position(1);
        }

        Ok(wrote)
    }
}

impl IntoIterator for WatchmanPendingChanges {
    type Item = Result<PendingChangeResult>;
    type IntoIter = std::vec::IntoIter<Self::Item>;

    fn into_iter(self) -> Self::IntoIter {
        self.pending_changes.into_iter()
    }
}

fn parse_watchman_pid(clock: Option<&Clock>) -> Option<u32> {
    match clock {
        Some(Clock::Spec(ClockSpec::StringClock(clock_str))) => match clock_str.split(':').nth(2) {
            None => None,
            Some(pid) => pid.parse().ok(),
        },
        _ => None,
    }
}
