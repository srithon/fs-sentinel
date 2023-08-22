use futures::StreamExt;
use futures::{future::join_all, stream::FuturesUnordered, Future};
use miniserde::Serialize;
use miniserde::{json, Deserialize};
use std::collections::HashMap;
use std::io;
use std::{path::PathBuf, pin::Pin};
use thiserror::Error;
use tokio::{
    fs,
    sync::{Mutex, RwLock},
};

mod platform;

use platform::{FileSystemWatcher, Platform};

#[cfg(feature = "linux")]
mod linux;

pub type FileSystemID = String;

#[derive(Error, Debug)]
pub enum FSSentinelError {
    #[error("couldn't read/write cache")]
    CacheError(#[from] io::Error),

    #[error("couldn't parse cache")]
    CacheParse(#[from] miniserde::Error),
}

#[derive(Clone, Debug)]
struct FileSystemState {
    /// the path given from the command-line, if any
    path: Option<PathBuf>,
    /// current modification status
    status: FileSystemModificationStatus,
    /// if currently has an active watcher in `filesystem_futures`, then `true`; otherwise `false`
    has_active_watcher: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum FileSystemModificationStatus {
    Modified,
    UnModified,
}

pub type Result<T> = std::result::Result<T, FSSentinelError>;

/// State maps filesystems to whether or not they were modified.
pub struct Daemon<P: Platform> {
    platform: P,
    filesystem_states: RwLock<HashMap<FileSystemID, Mutex<FileSystemState>>>,
    filesystem_futures: FuturesUnordered<Pin<Box<dyn Future<Output = FileSystemID>>>>,
}

/// A FileSystem combines a FileSystemID with a path contained within the FileSystem; for any
/// requirements concerning the path itself, see the documentation for your used `Platform`.
#[derive(Clone)]
pub struct FileSystem {
    pub id: FileSystemID,
    pub path: PathBuf,
}

/// Simplified cache structure for persisting in filesystem.
type Cache = HashMap<String, FileSystemModificationStatus>;

impl<P: Platform> Daemon<P> {
    /// Creates a clean Daemon instance without reading from the cache.
    pub fn new(platform: P) -> Self {
        Daemon {
            platform,
            filesystem_states: RwLock::new(HashMap::new()),
            filesystem_futures: FuturesUnordered::new(),
        }
    }

    pub fn from_cache(platform: P) -> Result<Self> {
        // now, let's read the cache
        let cache_contents = std::fs::read_to_string(platform.get_cache_path())?;

        let mut daemon = Self::new(platform);

        let deserialized_cache: Cache = json::from_str(&cache_contents)?;
        let processed_map = deserialized_cache
            .into_iter()
            .map(|(key, val)| {
                (
                    key,
                    Mutex::new(FileSystemState {
                        path: None,
                        status: val,
                        has_active_watcher: false,
                    }),
                )
            })
            .collect();

        daemon.filesystem_states = RwLock::new(processed_map);

        Ok(daemon)
    }

    async fn update_cache(&self) -> Result<()> {
        let rwlock_guard = self.filesystem_states.read().await;

        let value_futures = rwlock_guard
            .iter()
            .map(|(key, val)| {
                // source: https://users.rust-lang.org/t/how-to-use-await-inside-vec-iter-map-in-an-async-fn/65416/2
                async move {
                    let guard = val.lock().await;
                    (key, guard.status.clone())
                }
            })
            .collect::<Vec<_>>();

        // re-collect future results into a HashMap.
        let normalized_cache: Cache = join_all(value_futures)
            .await
            .into_iter()
            // this isn't actually needed for serialization, but to make the types match up (Cache
            // contains owned String's for deserialization purposes), we need to convert the
            // borrowed strings into owned values before collecting
            .map(|(key, value)| (key.to_owned(), value))
            .collect();

        let stringified_cache = json::to_string(&normalized_cache);

        // finally, write it to the filesystem
        fs::write(self.platform.get_cache_path(), stringified_cache).await?;

        Ok(())
    }

    async fn mark_filesystem_status(
        &mut self,
        fs_id: &FileSystemID,
        new_status: FileSystemModificationStatus,
    ) {
        let read_guard = self.filesystem_states.read().await;
        let mut fs_write_guard = read_guard
            .get(fs_id)
            .expect("FS must exist in map")
            .lock()
            .await;

        fs_write_guard.status = new_status.clone();

        use FileSystemModificationStatus::*;
        match new_status {
            Modified => (),
            UnModified => {
                // if we do not have an active process, then restart
                if !fs_write_guard.has_active_watcher {
                    let fs_path = fs_write_guard
                        .path
                        .clone()
                        .expect("FileSystem must have been specified in initial list!");

                    // explicitly drop guards so that we can mutate self.
                    drop(fs_write_guard);
                    drop(read_guard);

                    self.start_monitoring_filesystem(FileSystem {
                        id: fs_id.clone(),
                        path: fs_path,
                    })
                    .await;
                }
            }
        }
    }

    // TODO: don't require an owned `FileSystem`
    async fn start_monitoring_filesystem(&mut self, fs: FileSystem) {
        // if a filesystem is not present in our `statuses`, we will initialize it to `modified`
        let statuses_ro = self.filesystem_states.read().await;
        let filesystem_exists = statuses_ro.contains_key(&fs.id);

        if !filesystem_exists {
            let mut statuses_mutable = self.filesystem_states.write().await;

            statuses_mutable.insert(
                fs.id.clone(),
                Mutex::new(FileSystemState {
                    path: Some(fs.path.clone()),
                    status: FileSystemModificationStatus::Modified,
                    has_active_watcher: true,
                }),
            );
        } else {
            let mut state = statuses_ro
                .get(&fs.id)
                .expect("Must exist since we just checked")
                .lock()
                .await;

            state.has_active_watcher = true;
        }

        let watcher = self.platform.get_filesystem_watcher(fs);
        let future = watcher.wait_for_change();
        self.filesystem_futures.push(future);
    }

    /// When running the daemon, give it a list of paths to monitor, each with its own unique
    /// identifier which will be used as the cache key.
    pub async fn run(mut self, paths: Vec<FileSystem>) -> Result<()> {
        // to run the Daemon, we first start monitoring all filesystems
        for fs in paths {
            // OPTIMIZE: don't copy so much
            let fs_entry = self.filesystem_states.get_mut().entry(fs.id.clone());
            fs_entry
                .and_modify(|state| {
                    state.get_mut().path = Some(fs.path.clone());
                })
                .or_insert_with(|| {
                    Mutex::new(FileSystemState {
                        path: Some(fs.path.clone()),
                        status: FileSystemModificationStatus::UnModified,
                        has_active_watcher: false,
                    })
                });

            // OPTIMIZE: do asynchronously
            self.start_monitoring_filesystem(fs).await;
        }

        // now, let's do our main loop
        loop {
            let next_fs_id = self
                .filesystem_futures
                .next()
                .await
                .expect("Must not be None");

            // now, mark the filesystem as modified
            self.mark_filesystem_status(&next_fs_id, FileSystemModificationStatus::Modified)
                .await;
        }

        // Ok(())
    }
}
