/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::bail;
use anyhow::format_err;
use anyhow::Result;
use configmodel::convert::ByteCount;
use configmodel::Config;
use configmodel::ConfigExt;
use fs_err as fs;
use hgstore::strip_hg_file_metadata;
use hgtime::HgTime;
use minibytes::Bytes;
use tracing::info_span;
use types::Key;

use crate::datastore::ContentDataStore;
use crate::datastore::ContentMetadata;
use crate::datastore::Delta;
use crate::datastore::HgIdDataStore;
use crate::datastore::HgIdMutableDeltaStore;
use crate::datastore::LegacyStore;
use crate::datastore::Metadata;
use crate::datastore::RemoteDataStore;
use crate::datastore::StoreResult;
use crate::indexedlogdatastore::IndexedLogHgIdDataStore;
use crate::indexedlogdatastore::IndexedLogHgIdDataStoreConfig;
use crate::indexedlogutil::StoreType;
use crate::lfs::LfsClient;
use crate::lfs::LfsFallbackRemoteStore;
use crate::lfs::LfsMultiplexer;
use crate::lfs::LfsStore;
use crate::localstore::ExtStoredPolicy;
use crate::localstore::LocalStore;
use crate::packstore::CorruptionPolicy;
use crate::packstore::MutableDataPackStore;
use crate::remotestore::HgIdRemoteStore;
use crate::repack::RepackLocation;
use crate::types::StoreKey;
use crate::uniondatastore::UnionContentDataStore;
use crate::uniondatastore::UnionHgIdDataStore;
use crate::util::check_run_once;
use crate::util::get_cache_packs_path;
use crate::util::get_cache_path;
use crate::util::get_indexedlogdatastore_path;
use crate::util::get_local_path;
use crate::util::get_packs_path;
use crate::util::RUN_ONCE_FILENAME;

/// A `ContentStore` aggregate all the local and remote stores and expose them as one. Both local and
/// remote stores can be queried and accessed via the `HgIdDataStore` trait. The local store can also
/// be written to via the `HgIdMutableDeltaStore` trait, this is intended to be used to store local
/// commit data.
pub struct ContentStore {
    datastore: UnionHgIdDataStore<Arc<dyn HgIdDataStore>>,
    local_mutabledatastore: Option<Arc<dyn HgIdMutableDeltaStore>>,
    shared_mutabledatastore: Arc<dyn HgIdMutableDeltaStore>,
    remote_store: Option<Arc<dyn RemoteDataStore>>,

    blob_stores: UnionContentDataStore<Arc<dyn ContentDataStore>>,
}

impl ContentStore {
    pub fn new(local_path: impl AsRef<Path>, config: &dyn Config) -> Result<Self> {
        ContentStoreBuilder::new(config)
            .local_path(&local_path)
            .build()
    }
}

impl LegacyStore for ContentStore {
    /// Some blobs may contain copy-from metadata, let's strip it. For more details about the
    /// copy-from metadata, see `strip_hg_file_metadata`.
    ///
    /// XXX: This should only be used on `ContentStore` that are storing actual
    /// file content, tree stores should use the `get` method instead.
    fn get_file_content(&self, key: &Key) -> Result<Option<Bytes>> {
        if let StoreResult::Found(vec) = self.get(StoreKey::hgid(key.clone()))? {
            let bytes = vec.into();
            let (bytes, _) = strip_hg_file_metadata(&bytes)?;
            Ok(Some(bytes))
        } else {
            Ok(None)
        }
    }

    fn get_shared_mutable(&self) -> Arc<dyn HgIdMutableDeltaStore> {
        self.shared_mutabledatastore.clone()
    }

    // Repack specific methods, not to be used directly but by the repack code.
    fn add_pending(
        &self,
        key: &Key,
        data: Bytes,
        meta: Metadata,
        location: RepackLocation,
    ) -> Result<()> {
        let delta = Delta {
            data,
            base: None,
            key: key.clone(),
        };

        match location {
            RepackLocation::Local => self.add(&delta, &meta),
            RepackLocation::Shared => self.shared_mutabledatastore.add(&delta, &meta),
        }
    }

    fn commit_pending(&self, location: RepackLocation) -> Result<Option<Vec<PathBuf>>> {
        match location {
            RepackLocation::Local => self.flush(),
            RepackLocation::Shared => self.shared_mutabledatastore.flush(),
        }
    }
}

impl HgIdDataStore for ContentStore {
    fn get(&self, key: StoreKey) -> Result<StoreResult<Vec<u8>>> {
        self.datastore.get(key)
    }

    fn get_meta(&self, key: StoreKey) -> Result<StoreResult<Metadata>> {
        self.datastore.get_meta(key)
    }

    fn refresh(&self) -> Result<()> {
        self.datastore.refresh()
    }
}

impl RemoteDataStore for ContentStore {
    fn prefetch(&self, keys: &[StoreKey]) -> Result<Vec<StoreKey>> {
        if let Some(remote_store) = self.remote_store.as_ref() {
            let missing = self.get_missing(keys)?;
            if missing == vec![] {
                Ok(vec![])
            } else {
                remote_store.prefetch(&missing)
            }
        } else {
            // There is no remote store, let's pretend everything is fine.
            Ok(vec![])
        }
    }

    fn upload(&self, keys: &[StoreKey]) -> Result<Vec<StoreKey>> {
        if let Some(remote_store) = self.remote_store.as_ref() {
            remote_store.upload(keys)
        } else {
            Ok(keys.to_vec())
        }
    }
}

impl LocalStore for ContentStore {
    fn get_missing(&self, keys: &[StoreKey]) -> Result<Vec<StoreKey>> {
        let span = info_span!("Get Missing", keys = keys.len(),);
        span.in_scope(|| self.datastore.get_missing(keys))
    }
}

impl Drop for ContentStore {
    /// The shared store is a cache, so let's flush all pending data when the `ContentStore` goes
    /// out of scope.
    fn drop(&mut self) {
        let _ = self.shared_mutabledatastore.flush();
    }
}

/// HgIdMutableDeltaStore is only implemented for the local store and not for the remote ones. The
/// remote stores will be automatically written to while calling the various `HgIdDataStore` methods.
///
/// These methods can only be used when the ContentStore was created with a local store.
impl HgIdMutableDeltaStore for ContentStore {
    /// Add the data to the local store.
    fn add(&self, delta: &Delta, metadata: &Metadata) -> Result<()> {
        self.local_mutabledatastore
            .as_ref()
            .ok_or_else(|| format_err!("writing to a non-local ContentStore is not allowed"))?
            .add(delta, metadata)
    }

    /// Commit the data written to the local store.
    fn flush(&self) -> Result<Option<Vec<PathBuf>>> {
        self.shared_mutabledatastore.as_ref().flush()?;
        self.local_mutabledatastore
            .as_ref()
            .ok_or_else(|| format_err!("flushing a non-local ContentStore is not allowed"))?
            .flush()
    }
}

impl ContentDataStore for ContentStore {
    /// Fetch a raw blob from the LFS stores.
    fn blob(&self, key: StoreKey) -> Result<StoreResult<Bytes>> {
        self.blob_stores.blob(key)
    }

    fn metadata(&self, key: StoreKey) -> Result<StoreResult<ContentMetadata>> {
        self.blob_stores.metadata(key)
    }
}

/// Builder for `ContentStore`. An `impl AsRef<Path>` represents the path to the store and a
/// `dyn Config` of the Mercurial configuration are required to build a `ContentStore`. Users can
/// use this builder to add optional `HgIdRemoteStore` to enable remote data fetching， and a `Path`
/// suffix to specify other type of stores.
pub struct ContentStoreBuilder<'a> {
    local_path: Option<PathBuf>,
    no_local_store: bool,
    config: &'a dyn Config,
    remotestore: Option<Arc<dyn HgIdRemoteStore>>,
    suffix: Option<PathBuf>,
    shared_indexedlog_local: Option<Arc<IndexedLogHgIdDataStore>>,
    shared_indexedlog_shared: Option<Arc<IndexedLogHgIdDataStore>>,
    shared_lfs_local: Option<Arc<LfsStore>>,
    shared_lfs_shared: Option<Arc<LfsStore>>,
}

impl<'a> ContentStoreBuilder<'a> {
    pub fn new(config: &'a dyn Config) -> Self {
        Self {
            local_path: None,
            no_local_store: false,
            config,
            remotestore: None,
            suffix: None,
            shared_indexedlog_shared: None,
            shared_indexedlog_local: None,
            shared_lfs_shared: None,
            shared_lfs_local: None,
        }
    }

    /// Path to the local store.
    pub fn local_path(mut self, local_path: impl AsRef<Path>) -> Self {
        self.local_path = Some(local_path.as_ref().to_path_buf());
        self
    }

    /// Allows a ContentStore to be created without a local store.
    ///
    /// This should be used in very specific cases that do not want a local store. Unless you know
    /// exactly that this is what you want, do not use.
    pub fn no_local_store(mut self) -> Self {
        self.no_local_store = true;
        self
    }

    pub fn remotestore(mut self, remotestore: Arc<dyn HgIdRemoteStore>) -> Self {
        self.remotestore = Some(remotestore);
        self
    }

    pub fn suffix(mut self, suffix: impl AsRef<Path>) -> Self {
        self.suffix = Some(suffix.as_ref().to_path_buf());
        self
    }

    pub fn shared_indexedlog_local(mut self, indexedlog: Arc<IndexedLogHgIdDataStore>) -> Self {
        self.shared_indexedlog_local = Some(indexedlog);
        self
    }

    pub fn shared_indexedlog_shared(mut self, indexedlog: Arc<IndexedLogHgIdDataStore>) -> Self {
        self.shared_indexedlog_shared = Some(indexedlog);
        self
    }

    pub fn shared_lfs_local(mut self, lfs: Arc<LfsStore>) -> Self {
        self.shared_lfs_local = Some(lfs);
        self
    }

    pub fn shared_lfs_shared(mut self, lfs: Arc<LfsStore>) -> Self {
        self.shared_lfs_shared = Some(lfs);
        self
    }

    pub fn build(self) -> Result<ContentStore> {
        let local_path = self
            .local_path
            .as_ref()
            .map(|p| get_local_path(p.clone(), &self.suffix))
            .transpose()?;
        let cache_path = get_cache_path(self.config, &self.suffix)?;

        if let Some(cache_path) = cache_path.as_ref() {
            check_cache_buster(&self.config, cache_path);
        }

        // Do this after the cache busting, since this will recreate the necessary directories.
        let cache_packs_path = get_cache_packs_path(self.config, &self.suffix)?;
        let max_pending_bytes = self
            .config
            .get_or("packs", "maxdatapendingbytes", || {
                // Default to 4GB
                ByteCount::from(4 * (1024 * 1024 * 1024))
            })?
            .value();
        let max_bytes = self
            .config
            .get_opt::<ByteCount>("packs", "maxdatabytes")?
            .map(|v| v.value());

        let mut datastore: UnionHgIdDataStore<Arc<dyn HgIdDataStore>> = UnionHgIdDataStore::new();
        let mut blob_stores: UnionContentDataStore<Arc<dyn ContentDataStore>> =
            UnionContentDataStore::new();

        let enable_lfs = self.config.get_or_default::<bool>("remotefilelog", "lfs")?;
        let extstored_policy = if enable_lfs {
            if self
                .config
                .get_or_default::<bool>("remotefilelog", "useextstored")?
            {
                ExtStoredPolicy::Use
            } else {
                ExtStoredPolicy::Ignore
            }
        } else {
            ExtStoredPolicy::Use
        };

        let shared_pack_store = match cache_packs_path {
            Some(path) => Some(Arc::new(MutableDataPackStore::new(
                path,
                CorruptionPolicy::REMOVE,
                max_pending_bytes,
                max_bytes,
                extstored_policy,
            )?)),
            None => None,
        };

        let shared_indexedlogdatastore =
            if let Some(shared_indexedlog_shared) = self.shared_indexedlog_shared {
                Some(shared_indexedlog_shared)
            } else if let Some(cache_path) = cache_path.as_ref() {
                let max_log_count = self
                    .config
                    .get_opt::<u8>("indexedlog", "data.max-log-count")?;
                let max_bytes_per_log = self
                    .config
                    .get_opt::<ByteCount>("indexedlog", "data.max-bytes-per-log")?;
                let max_bytes = self
                    .config
                    .get_opt::<ByteCount>("remotefilelog", "cachelimit")?;
                let config = IndexedLogHgIdDataStoreConfig {
                    max_log_count,
                    max_bytes_per_log,
                    max_bytes,
                };
                Some(Arc::new(IndexedLogHgIdDataStore::new(
                    self.config,
                    get_indexedlogdatastore_path(cache_path)?,
                    extstored_policy,
                    &config,
                    StoreType::Rotated,
                )?))
            } else {
                None
            };

        // The shared stores should precede the local one since we expect both the number of blobs,
        // and the number of requests satisfied by the shared cache to be significantly higher than
        // ones in the local store.

        let lfs_threshold = if enable_lfs {
            self.config.get_opt::<ByteCount>("lfs", "threshold")?
        } else {
            None
        };

        let shared_lfs_store = if let Some(shared_lfs_shared) = self.shared_lfs_shared {
            Some(shared_lfs_shared)
        } else if let Some(cache_path) = cache_path {
            Some(Arc::new(LfsStore::rotated(&cache_path, self.config)?))
        } else {
            None
        };

        if let Some(shared_lfs_store) = shared_lfs_store.clone() {
            blob_stores.add(shared_lfs_store);
        }

        let primary: Option<Arc<dyn HgIdMutableDeltaStore>> =
            if self
                .config
                .get_or("remotefilelog", "write-hgcache-to-indexedlog", || true)?
            {
                // Put the indexedlog first, since recent data will have gone there.
                if let Some(shared_indexedlogdatastore) = shared_indexedlogdatastore.clone() {
                    datastore.add(shared_indexedlogdatastore);
                }
                if let Some(shared_pack_store) = shared_pack_store {
                    datastore.add(shared_pack_store);
                }
                shared_indexedlogdatastore.map(|store| store as Arc<dyn HgIdMutableDeltaStore>)
            } else {
                if let Some(shared_pack_store) = shared_pack_store.clone() {
                    datastore.add(shared_pack_store);
                }
                if let Some(shared_indexedlogdatastore) = shared_indexedlogdatastore.clone() {
                    datastore.add(shared_indexedlogdatastore);
                }
                shared_pack_store.map(|store| store as Arc<dyn HgIdMutableDeltaStore>)
            };

        if let Some(shared_lfs_store) = shared_lfs_store.clone() {
            datastore.add(shared_lfs_store);
        }

        let shared_mutabledatastore: Option<Arc<dyn HgIdMutableDeltaStore>> =
            if let Some(lfs_threshold) = lfs_threshold {
                if let (Some(shared), Some(primary)) = (shared_lfs_store.clone(), primary.clone()) {
                    Some(Arc::new(LfsMultiplexer::new(
                        shared,
                        primary,
                        lfs_threshold.value() as usize,
                    )) as _)
                } else {
                    None
                }
            } else {
                primary
            };

        let (local_mutabledatastore, local_lfs_store): (Option<Arc<dyn HgIdMutableDeltaStore>>, _) =
            if let Some(unsuffixed_local_path) = self.local_path {
                let local_pack_store = Arc::new(MutableDataPackStore::new(
                    get_packs_path(unsuffixed_local_path, &self.suffix)?,
                    CorruptionPolicy::IGNORE,
                    max_pending_bytes,
                    None,
                    extstored_policy,
                )?);
                let local_indexedlogdatastore =
                    if let Some(shared_indexedlog_local) = self.shared_indexedlog_local {
                        shared_indexedlog_local
                    } else {
                        let config = IndexedLogHgIdDataStoreConfig {
                            max_log_count: None,
                            max_bytes_per_log: None,
                            max_bytes: None,
                        };
                        Arc::new(IndexedLogHgIdDataStore::new(
                            self.config,
                            get_indexedlogdatastore_path(local_path.as_ref().unwrap())?,
                            extstored_policy,
                            &config,
                            StoreType::Permanent,
                        )?)
                    };

                let primary: Arc<dyn HgIdMutableDeltaStore> =
                    if self
                        .config
                        .get_or("remotefilelog", "write-local-to-indexedlog", || true)?
                    {
                        // Put the indexedlog first, since recent data will have gone there.
                        datastore.add(local_indexedlogdatastore.clone());
                        datastore.add(local_pack_store);
                        local_indexedlogdatastore
                    } else {
                        datastore.add(local_pack_store.clone());
                        datastore.add(local_indexedlogdatastore);
                        local_pack_store
                    };

                let local_lfs_store = if let Some(shared_lfs_local) = self.shared_lfs_local {
                    shared_lfs_local
                } else {
                    Arc::new(LfsStore::permanent(local_path.unwrap(), self.config)?)
                };
                blob_stores.add(local_lfs_store.clone());
                datastore.add(local_lfs_store.clone());

                let local_mutabledatastore: Arc<dyn HgIdMutableDeltaStore> = {
                    if let Some(lfs_threshold) = lfs_threshold {
                        Arc::new(LfsMultiplexer::new(
                            local_lfs_store.clone(),
                            primary,
                            lfs_threshold.value() as usize,
                        ))
                    } else {
                        primary
                    }
                };

                (Some(local_mutabledatastore), Some(local_lfs_store))
            } else {
                if !self.no_local_store {
                    return Err(format_err!(
                        "a ContentStore cannot be built without a local store"
                    ));
                }
                (None, None)
            };

        let shared_mutabledatastore = match shared_mutabledatastore {
            Some(store) => store,
            None => match local_mutabledatastore.clone() {
                Some(store) => store,
                None => bail!("ContentStore requires at least one of local store or shared store"),
            },
        };

        let remote_store: Option<Arc<dyn RemoteDataStore>> =
            if let Some(remotestore) = self.remotestore {
                let shared_store = shared_mutabledatastore.clone();
                let mut remotestores = UnionHgIdDataStore::new();

                // Add remotestore. For LFS blobs, the LFS pointers will be fetched
                // at this step and be written to the LFS store.
                let filenode_remotestore = remotestore.datastore(shared_store.clone());
                remotestores.add(filenode_remotestore.clone());

                // Third, the LFS remote store. The previously fetched LFS pointers will be used to
                // fetch the actual blobs in this store.
                if enable_lfs {
                    if let Some(shared_lfs_store) = shared_lfs_store {
                        let lfs_remote_store = Arc::new(LfsClient::new(
                            shared_lfs_store,
                            local_lfs_store,
                            self.config,
                        )?);
                        remotestores.add(lfs_remote_store.datastore(shared_store.clone()));
                    }

                    // Fallback store if the LFS one is dead.
                    let lfs_fallback = LfsFallbackRemoteStore::new(filenode_remotestore);
                    remotestores.add(lfs_fallback);
                }

                let remotestores: Box<dyn RemoteDataStore> = Box::new(remotestores);
                let remotestores = Arc::new(remotestores);
                datastore.add(remotestores.clone());
                Some(remotestores)
            } else {
                None
            };

        Ok(ContentStore {
            datastore,
            local_mutabledatastore,
            shared_mutabledatastore,
            remote_store,
            blob_stores,
        })
    }
}

/// Reads the configs and deletes the hgcache if a hgcache-purge.$KEY=$DATE value hasn't already
/// been processed.
pub fn check_cache_buster(config: &dyn Config, store_path: &Path) {
    for key in config.keys("hgcache-purge").into_iter() {
        if let Some(cutoff) = config
            .get("hgcache-purge", &key)
            .and_then(|c| HgTime::parse(&c))
        {
            if check_run_once(store_path, &key, cutoff) {
                let _ = delete_hgcache(store_path);
                break;
            }
        }
    }
}

/// Recursively deletes the contents of the path, excluding the run-once marker file.
/// Ignores errors on individual files or directories.
fn delete_hgcache(store_path: &Path) -> Result<()> {
    for file in fs::read_dir(store_path)? {
        let _ = (|| -> Result<()> {
            let file = file?;
            if file.file_name() == RUN_ONCE_FILENAME {
                return Ok(());
            }

            let path = file.path();
            let file_type = file.file_type()?;
            if file_type.is_dir() {
                fs::remove_dir_all(path)?;
            } else if file_type.is_file() || file_type.is_symlink() {
                fs::remove_file(path)?;
            }
            Ok(())
        })();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::ffi::OsString;
    use std::ops::Add;
    use std::ops::Sub;

    use minibytes::Bytes;
    #[cfg(feature = "fb")]
    use mockito::Mock;
    use tempfile::TempDir;
    use types::testutil::*;
    use util::path::create_dir;

    use super::*;
    use crate::metadatastore::MetadataStore;
    use crate::repack::repack;
    use crate::repack::RepackKind;
    use crate::repack::RepackLocation;
    #[cfg(feature = "fb")]
    use crate::testutil::example_blob;
    #[cfg(feature = "fb")]
    use crate::testutil::get_lfs_batch_mock;
    #[cfg(feature = "fb")]
    use crate::testutil::get_lfs_download_mock;
    use crate::testutil::make_config;
    use crate::testutil::make_lfs_config;
    use crate::testutil::setconfig;
    use crate::testutil::FakeHgIdRemoteStore;
    #[cfg(feature = "fb")]
    use crate::testutil::TestBlob;
    use crate::types::ContentHash;

    #[cfg(feature = "fb")]
    fn prepare_lfs_mocks(server: &mut mockito::ServerGuard, blob: &TestBlob) -> Vec<Mock> {
        let m1 = get_lfs_batch_mock(server, 200, &[blob]);
        let mut m2 = get_lfs_download_mock(server, 200, blob);
        m2.push(m1);
        m2
    }

    #[test]
    fn test_new() -> Result<()> {
        let cachedir = TempDir::new()?;
        let localdir = TempDir::new()?;
        let config = make_config(&cachedir);

        let _store = ContentStore::new(&localdir, &config)?;
        Ok(())
    }

    #[test]
    fn test_add_get() -> Result<()> {
        let cachedir = TempDir::new()?;
        let localdir = TempDir::new()?;
        let config = make_config(&cachedir);

        let store = ContentStore::new(&localdir, &config)?;

        let k1 = key("a", "2");
        let delta = Delta {
            data: Bytes::from(&[1, 2, 3, 4][..]),
            base: None,
            key: k1.clone(),
        };
        store.add(&delta, &Default::default())?;
        let stored = store.get(StoreKey::hgid(k1))?;
        assert_eq!(stored, StoreResult::Found(delta.data.as_ref().to_vec()));
        Ok(())
    }

    #[test]
    fn test_add_dropped() -> Result<()> {
        let cachedir = TempDir::new()?;
        let localdir = TempDir::new()?;
        let mut config = make_config(&cachedir);
        setconfig(
            &mut config,
            "remotefilelog",
            "write-local-to-indexedlog",
            "False",
        );

        let store = ContentStore::new(&localdir, &config)?;

        let k1 = key("a", "2");
        let delta = Delta {
            data: Bytes::from(&[1, 2, 3, 4][..]),
            base: Some(key("a", "1")),
            key: k1.clone(),
        };
        store.add(&delta, &Default::default())?;
        drop(store);

        let store = ContentStore::new(&localdir, &config)?;
        let k1 = StoreKey::hgid(k1);
        assert_eq!(store.get(k1.clone())?, StoreResult::NotFound(k1));
        Ok(())
    }

    #[test]
    fn test_add_flush_get() -> Result<()> {
        let cachedir = TempDir::new()?;
        let localdir = TempDir::new()?;
        let config = make_config(&cachedir);

        let store = ContentStore::new(&localdir, &config)?;

        let k1 = key("a", "2");
        let delta = Delta {
            data: Bytes::from(&[1, 2, 3, 4][..]),
            base: None,
            key: k1.clone(),
        };
        store.add(&delta, &Default::default())?;
        store.flush()?;
        let stored = store.get(StoreKey::hgid(k1))?;
        assert_eq!(stored, StoreResult::Found(delta.data.as_ref().to_vec()));
        Ok(())
    }

    #[test]
    fn test_add_flush_drop_get() -> Result<()> {
        let cachedir = TempDir::new()?;
        let localdir = TempDir::new()?;
        let config = make_config(&cachedir);

        let store = ContentStore::new(&localdir, &config)?;

        let k1 = key("a", "2");
        let delta = Delta {
            data: Bytes::from(&[1, 2, 3, 4][..]),
            base: None,
            key: k1.clone(),
        };
        store.add(&delta, &Default::default())?;
        store.flush()?;
        drop(store);

        let store = ContentStore::new(&localdir, &config)?;
        let stored = store.get(StoreKey::hgid(k1))?;
        assert_eq!(stored, StoreResult::Found(delta.data.as_ref().to_vec()));
        Ok(())
    }

    #[test]
    fn test_remote_store() -> Result<()> {
        let cachedir = TempDir::new()?;
        let localdir = TempDir::new()?;
        let config = make_config(&cachedir);

        let k = key("a", "1");
        let data = Bytes::from(&[1, 2, 3, 4][..]);

        let mut map = HashMap::new();
        map.insert(k.clone(), (data.clone(), None));
        let mut remotestore = FakeHgIdRemoteStore::new();
        remotestore.data(map);

        let store = ContentStoreBuilder::new(&config)
            .local_path(&localdir)
            .remotestore(Arc::new(remotestore))
            .build()?;
        let data_get = store.get(StoreKey::hgid(k))?;

        assert_eq!(data_get, StoreResult::Found(data.as_ref().to_vec()));
        Ok(())
    }

    #[test]
    fn test_remote_store_cached() -> Result<()> {
        let cachedir = TempDir::new()?;
        let localdir = TempDir::new()?;
        let config = make_config(&cachedir);

        let k = key("a", "1");
        let data = Bytes::from(&[1, 2, 3, 4][..]);

        let mut map = HashMap::new();
        map.insert(k.clone(), (data.clone(), None));

        let mut remotestore = FakeHgIdRemoteStore::new();
        remotestore.data(map);

        let store = ContentStoreBuilder::new(&config)
            .local_path(&localdir)
            .remotestore(Arc::new(remotestore))
            .build()?;
        store.get(StoreKey::hgid(k.clone()))?;
        drop(store);

        let store = ContentStore::new(&localdir, &config)?;
        let data_get = store.get(StoreKey::hgid(k))?;

        assert_eq!(data_get, StoreResult::Found(data.as_ref().to_vec()));

        Ok(())
    }

    #[test]
    fn test_not_in_remote_store() -> Result<()> {
        let cachedir = TempDir::new()?;
        let localdir = TempDir::new()?;
        let config = make_config(&cachedir);

        let map = HashMap::new();
        let mut remotestore = FakeHgIdRemoteStore::new();
        remotestore.data(map);

        let store = ContentStoreBuilder::new(&config)
            .local_path(&localdir)
            .remotestore(Arc::new(remotestore))
            .build()?;

        let k = StoreKey::hgid(key("a", "1"));
        assert_eq!(store.get(k.clone())?, StoreResult::NotFound(k));
        Ok(())
    }

    #[test]
    fn test_local_indexedlog_write() -> Result<()> {
        let cachedir = TempDir::new()?;
        let localdir = TempDir::new()?;
        let mut config = make_config(&cachedir);
        setconfig(
            &mut config,
            "remotefilelog",
            "write-local-to-indexedlog",
            "True",
        );

        let store = ContentStoreBuilder::new(&config)
            .local_path(&localdir)
            .build()?;

        let k1 = key("a", "2");
        let delta = Delta {
            data: Bytes::from(&[1, 2, 3, 4][..]),
            base: None,
            key: k1.clone(),
        };
        store.add(&delta, &Default::default())?;
        store.flush()?;
        drop(store);

        let indexed_log_config = IndexedLogHgIdDataStoreConfig {
            max_log_count: None,
            max_bytes_per_log: None,
            max_bytes: None,
        };
        let store = IndexedLogHgIdDataStore::new(
            &config,
            get_indexedlogdatastore_path(&localdir)?,
            ExtStoredPolicy::Use,
            &indexed_log_config,
            StoreType::Permanent,
        )?;
        assert_eq!(
            store.get(StoreKey::hgid(k1))?,
            StoreResult::Found(delta.data.as_ref().to_vec())
        );
        Ok(())
    }

    #[test]
    fn test_fetch_location() -> Result<()> {
        let cachedir = TempDir::new()?;
        let localdir = TempDir::new()?;
        let config = make_config(&cachedir);

        let k = key("a", "1");
        let data = Bytes::from(&[1, 2, 3, 4][..]);

        let mut map = HashMap::new();
        map.insert(k.clone(), (data, None));

        let mut remotestore = FakeHgIdRemoteStore::new();
        remotestore.data(map);

        let store = ContentStoreBuilder::new(&config)
            .local_path(&localdir)
            .remotestore(Arc::new(remotestore))
            .build()?;
        store.get(StoreKey::hgid(k.clone()))?;
        store
            .shared_mutabledatastore
            .get(StoreKey::hgid(k.clone()))?;
        let k = StoreKey::hgid(k);
        let res = store
            .local_mutabledatastore
            .as_ref()
            .unwrap()
            .get(k.clone())?;
        assert_eq!(res, StoreResult::NotFound(k));
        Ok(())
    }

    #[test]
    fn test_add_shared_only_store() -> Result<()> {
        let cachedir = TempDir::new()?;
        let localdir = TempDir::new()?;
        let mut config = make_config(&cachedir);
        setconfig(
            &mut config,
            "remotefilelog",
            "write-local-to-indexedlog",
            "False",
        );

        let store = ContentStore::new(&localdir, &config)?;

        let k1 = key("a", "2");
        let delta = Delta {
            data: Bytes::from(&[1, 2, 3, 4][..]),
            base: Some(key("a", "1")),
            key: k1.clone(),
        };
        store.add(&delta, &Default::default())?;
        store.flush()?;

        let store = ContentStoreBuilder::new(&config).no_local_store().build()?;
        let k = StoreKey::hgid(k1);
        assert_eq!(store.get(k.clone())?, StoreResult::NotFound(k));
        Ok(())
    }

    #[test]
    fn test_no_local_store() -> Result<()> {
        let cachedir = TempDir::new()?;
        let config = make_config(&cachedir);
        assert!(ContentStoreBuilder::new(&config).build().is_err());
        Ok(())
    }

    #[test]
    fn test_lfs_local() -> Result<()> {
        let cachedir = TempDir::new()?;
        let localdir = TempDir::new()?;
        let config = make_config(&cachedir);

        let lfs_store = LfsStore::permanent(&localdir, &config)?;
        let k1 = key("a", "2");
        let delta = Delta {
            data: Bytes::from(&[1, 2, 3, 4][..]),
            base: None,
            key: k1.clone(),
        };
        lfs_store.add(&delta, &Default::default())?;
        lfs_store.flush()?;

        let store = ContentStore::new(&localdir, &config)?;
        assert_eq!(
            store.get(StoreKey::hgid(k1))?,
            StoreResult::Found(delta.data.as_ref().to_vec())
        );
        Ok(())
    }

    #[test]
    fn test_lfs_shared() -> Result<()> {
        let cachedir = TempDir::new()?;
        let localdir = TempDir::new()?;
        let config = make_config(&cachedir);

        let mut lfs_cache_dir = cachedir.path().to_path_buf();
        lfs_cache_dir.push("test");
        create_dir(&lfs_cache_dir)?;
        let lfs_store = LfsStore::rotated(&lfs_cache_dir, &config)?;
        let k1 = key("a", "2");
        let delta = Delta {
            data: Bytes::from(&[1, 2, 3, 4][..]),
            base: None,
            key: k1.clone(),
        };
        lfs_store.add(&delta, &Default::default())?;
        lfs_store.flush()?;

        let store = ContentStore::new(&localdir, &config)?;
        assert_eq!(
            store.get(StoreKey::hgid(k1))?,
            StoreResult::Found(delta.data.as_ref().to_vec())
        );
        Ok(())
    }

    #[test]
    fn test_lfs_blob() -> Result<()> {
        let cachedir = TempDir::new()?;
        let localdir = TempDir::new()?;
        let server = mockito::Server::new();
        let config = make_lfs_config(&server, &cachedir, "test_lfs_blob");

        let k1 = key("a", "2");
        let delta = Delta {
            data: Bytes::from(&[1, 2, 3, 4, 5][..]),
            base: None,
            key: k1.clone(),
        };

        let store = ContentStore::new(&localdir, &config)?;
        store.add(&delta, &Default::default())?;

        let blob = store.blob(StoreKey::from(k1))?;
        assert_eq!(blob, StoreResult::Found(delta.data));

        Ok(())
    }

    #[test]
    fn test_lfs_metadata() -> Result<()> {
        let cachedir = TempDir::new()?;
        let localdir = TempDir::new()?;
        let server = mockito::Server::new();
        let config = make_lfs_config(&server, &cachedir, "test_lfs_metadata");

        let k1 = key("a", "2");
        let data = Bytes::from(&[1, 2, 3, 4, 5][..]);
        let hash = ContentHash::sha256(&data);
        let delta = Delta {
            data,
            base: None,
            key: k1.clone(),
        };

        let store = ContentStore::new(&localdir, &config)?;
        store.add(&delta, &Default::default())?;

        let metadata = store.metadata(StoreKey::from(k1))?;
        assert_eq!(
            metadata,
            StoreResult::Found(ContentMetadata {
                size: 5,
                is_binary: false,
                hash,
            })
        );

        Ok(())
    }

    #[test]
    fn test_lfs_multiplexer() -> Result<()> {
        let cachedir = TempDir::new()?;
        let localdir = TempDir::new()?;
        let server = mockito::Server::new();
        let config = make_lfs_config(&server, &cachedir, "test_lfs_multiplexer");

        let k1 = key("a", "2");
        let delta = Delta {
            data: Bytes::from(&[1, 2, 3, 4, 5][..]),
            base: None,
            key: k1.clone(),
        };

        let store = ContentStore::new(&localdir, &config)?;
        store.add(&delta, &Default::default())?;
        store.flush()?;

        let lfs_store = LfsStore::permanent(&localdir, &config)?;
        let stored = lfs_store.get(StoreKey::hgid(k1))?;
        assert_eq!(stored, StoreResult::Found(delta.data.as_ref().to_vec()));
        Ok(())
    }

    #[test]
    fn test_repack_one_datapack_lfs() -> Result<()> {
        let cachedir = TempDir::new()?;
        let localdir = TempDir::new()?;
        let server = mockito::Server::new();
        let mut config = make_lfs_config(&server, &cachedir, "test_repack_one_datapack_lfs");
        setconfig(&mut config, "lfs", "threshold", "10M");

        let k1 = key("a", "2");
        let delta = Delta {
            data: Bytes::from(&[1, 2, 3, 4, 5][..]),
            base: None,
            key: k1.clone(),
        };

        let store = Arc::new(ContentStore::new(&localdir, &config)?);
        store.add(&delta, &Default::default())?;
        store.flush()?;

        let metadata = Arc::new(MetadataStore::new(&localdir, &config)?);

        repack(
            get_packs_path(&localdir, &None)?,
            Some((store, metadata)),
            RepackKind::Full,
            RepackLocation::Local,
            &config,
        )?;

        let store = Arc::new(ContentStore::new(&localdir, &config)?);
        let stored = store.get(StoreKey::hgid(k1))?;
        assert_eq!(stored, StoreResult::Found(delta.data.as_ref().to_vec()));
        Ok(())
    }

    #[test]
    fn test_purge_cache() -> Result<()> {
        let cachedir = TempDir::new()?;
        let localdir = TempDir::new()?;
        let mut config = make_config(&cachedir);
        setconfig(
            &mut config,
            "remotefilelog",
            "write-local-to-indexedlog",
            "False",
        );
        setconfig(
            &mut config,
            "remotefilelog",
            "write-hgcache-to-indexedlog",
            "False",
        );

        let k = key("a", "2");
        let store_key = StoreKey::hgid(k.clone());
        let data = Bytes::from(&[1, 2, 3, 4, 5][..]);

        let mut map = HashMap::new();
        map.insert(k, (data.clone(), None));
        let mut remotestore = FakeHgIdRemoteStore::new();
        remotestore.data(map);
        let remotestore = Arc::new(remotestore);

        let create_store = |config: &dyn Config| -> ContentStore {
            ContentStoreBuilder::new(&config)
                .local_path(&localdir)
                .remotestore(remotestore.clone())
                .build()
                .unwrap()
        };

        // Populate the cache
        let store = create_store(&mut config);
        let stored = store.get(store_key.clone())?;
        assert_eq!(stored, StoreResult::Found(data.as_ref().to_vec()));

        // Drop the store so any temp files (for mutable packs) are deleted.
        drop(store);

        let get_subdirs = || -> Vec<OsString> {
            fs::read_dir(cachedir.path().join("test/packs"))
                .unwrap()
                .map(|f| f.unwrap().file_name())
                .collect::<Vec<_>>()
        };

        // Ensure pack files exist
        assert!(!get_subdirs().is_empty());

        // Set a purge that ended yesterday.
        let yesterday = HgTime::now().unwrap().sub(86000).unwrap();
        setconfig(
            &mut config,
            "hgcache-purge",
            "marker",
            &yesterday.to_utc().to_string(),
        );

        // Recreate the store, which should not activate the purge.
        let store = create_store(&mut config);
        drop(store);

        assert!(!get_subdirs().is_empty());

        // Set a purge that lasts until tomorrow.
        let tomorrow = HgTime::now().unwrap().add(86000).unwrap();
        setconfig(
            &mut config,
            "hgcache-purge",
            "marker",
            &tomorrow.to_utc().to_string(),
        );

        // Recreate the store, which will activate the purge.
        let store = create_store(&mut config);
        drop(store);

        assert!(get_subdirs().is_empty());

        // Populate the store again
        let store = create_store(&mut config);
        let _ = store.get(store_key)?;

        // Construct a store again and verify it doesn't purge the cache
        let store = create_store(&mut config);
        drop(store);

        assert!(!get_subdirs().is_empty());
        Ok(())
    }

    #[cfg(feature = "fb")]
    mod fb_tests {
        use std::str::FromStr;

        use types::Sha256;
        use url::Url;

        use super::*;

        #[test]
        fn test_lfs_remote() -> Result<()> {
            let _env_lock = crate::env_lock();

            let cachedir = TempDir::new()?;
            let localdir = TempDir::new()?;
            let mut server = mockito::Server::new();
            let config = make_lfs_config(&server, &cachedir, "test_lfs_remote");
            let blob = example_blob();
            let _lfs_mocks = prepare_lfs_mocks(&mut server, &blob);

            let k = key("a", "1");

            let pointer = format!(
                "version https://git-lfs.github.com/spec/v1\noid sha256:{}\nsize {}\nx-is-binary 0\n",
                blob.sha.to_hex(),
                blob.size,
            );

            let data = Bytes::from(pointer);

            let mut map = HashMap::new();
            map.insert(k.clone(), (data, Some(0x2000)));
            let mut remotestore = FakeHgIdRemoteStore::new();
            remotestore.data(map);

            let store = ContentStoreBuilder::new(&config)
                .local_path(&localdir)
                .remotestore(Arc::new(remotestore))
                .build()?;

            let data = store.get(StoreKey::hgid(k))?;

            assert_eq!(
                data,
                StoreResult::Found(Bytes::from(&b"master"[..]).as_ref().to_vec())
            );

            Ok(())
        }

        #[test]
        fn test_lfs_fallback_on_missing_blob() -> Result<()> {
            let cachedir = TempDir::new()?;
            let localdir = TempDir::new()?;
            let server = mockito::Server::new();
            let mut config =
                make_lfs_config(&server, &cachedir, "test_lfs_fallback_on_missing_blob");

            let lfsdir = TempDir::new()?;
            setconfig(
                &mut config,
                "lfs",
                "url",
                Url::from_file_path(&lfsdir).unwrap().as_ref(),
            );

            let k = key("a", "1");
            // This should be a missing blob.
            let sha256 = Sha256::from_str(
                "0000000000000000000000000000000000000000000000000000000000000042",
            )?;
            let size = 4;

            let pointer = format!(
                "version https://git-lfs.github.com/spec/v1\noid sha256:{}\nsize {}\nx-is-binary 0\n",
                sha256.to_hex(),
                size
            );

            let data = Bytes::from("AAAA");

            let mut map = HashMap::new();
            map.insert(k.clone(), (data, None));
            let mut remotestore = FakeHgIdRemoteStore::new();
            remotestore.data(map);

            let store = ContentStoreBuilder::new(&config)
                .local_path(&localdir)
                .remotestore(Arc::new(remotestore))
                .build()?;

            let delta = Delta {
                data: Bytes::from(pointer),
                base: None,
                key: k.clone(),
            };

            // Add the pointer the the shared store, but not the blob
            store.shared_mutabledatastore.add(
                &delta,
                &Metadata {
                    size: None,
                    flags: Some(0x2000),
                },
            )?;

            assert_eq!(
                store.get_missing(&[StoreKey::from(k.clone())])?,
                vec![StoreKey::Content(
                    ContentHash::Sha256(sha256),
                    Some(k.clone())
                )]
            );
            store.prefetch(&[StoreKey::from(k.clone())])?;
            // Even though the blob was missing, we got it!
            assert_eq!(store.get_missing(&[StoreKey::from(k)])?, vec![]);

            Ok(())
        }

        #[test]
        fn test_lfs_prefetch_once() -> Result<()> {
            let _env_lock = crate::env_lock();
            let blob = example_blob();
            let mut server = mockito::Server::new();
            let _lfs_mocks = prepare_lfs_mocks(&mut server, &blob);

            let cachedir = TempDir::new()?;
            let localdir = TempDir::new()?;
            let config = make_lfs_config(&server, &cachedir, "test_lfs_prefetch_once");

            let k1 = key("a", "1");
            let k2 = key("a", "2");
            let sha256 = Sha256::from_str(
                "fc613b4dfd6736a7bd268c8a0e74ed0d1c04a959f59dd74ef2874983fd443fc9",
            )?;
            let size = 6;

            let pointer = format!(
                "version https://git-lfs.github.com/spec/v1\noid sha256:{}\nsize {}\nx-is-binary 0\n",
                sha256.to_hex(),
                size
            );

            let data = Bytes::from(pointer);

            let mut map = HashMap::new();
            map.insert(k1.clone(), (data.clone(), Some(0x2000)));
            map.insert(k2.clone(), (data, Some(0x2000)));
            let mut remotestore = FakeHgIdRemoteStore::new();
            remotestore.data(map);

            let store = ContentStoreBuilder::new(&config)
                .local_path(&localdir)
                .remotestore(Arc::new(remotestore))
                .build()?;

            let k1 = StoreKey::from(k1);
            let k2 = StoreKey::from(k2);
            assert_eq!(store.prefetch(&[k1, k2])?, vec![]);

            Ok(())
        }
    }
}
