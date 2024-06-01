use futures::{future::join_all, stream::FuturesUnordered, Future};
use futures::{FutureExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::{path::PathBuf, pin::Pin};
use tokio::net::UnixListener;
use tokio_stream::wrappers::UnixListenerStream;

use tokio::signal::ctrl_c;
use tokio::signal::unix::{signal, SignalKind};
use tokio::{
    fs,
    io::{AsyncReadExt, AsyncWriteExt},
    sync::{Mutex, RwLock},
};

use crate::{
    platform::{FileSystemWatcher, Platform},
    wrap_err, FSSentinelError, FileSystem, FileSystemID, FileSystemModificationStatus, Result,
};

// unfortunately, we can't create a const Path as of this writing; see
// https://github.com/rust-lang/rust/pull/92930.
pub const SOCKET_PATH: &'static str = "/tmp/fs-sentinel.sock";

#[derive(Clone, Debug)]
struct FileSystemState {
    /// the path given from the command-line, if any
    path: Option<PathBuf>,
    /// current modification status
    status: FileSystemModificationStatus,
    /// if currently has an active watcher in `filesystem_futures`, then `true`; otherwise `false`
    has_active_watcher: bool,
    /// if the filesystem was passed into [Daemon::run], then `true`; otherwise `false`
    is_daemon_monitoring: bool,
}

/// State maps filesystems to whether or not they were modified.
pub struct Daemon<P: Platform> {
    platform: P,
    filesystem_states: RwLock<HashMap<FileSystemID, Mutex<FileSystemState>>>,
    filesystem_futures: FuturesUnordered<Pin<Box<dyn Future<Output = FileSystemID>>>>,
}

/// Allowed IPC messages for communicating with the Daemon
#[derive(Serialize, Deserialize, Debug)]
pub enum IPCInput {
    GetFileSystemStatus(FileSystemID),
    MarkFileSystemUnModified(FileSystemID),
    ListAllModifiedFileSystems,
}

/// Potential output responses for IPCInput messages
#[derive(Serialize, Deserialize, Debug)]
pub enum IPCOutput {
    DaemonError(String),
    Success,
    FileSystemStatus(FileSystemModificationStatus),
    FileSystemList(Vec<FileSystemID>),
}

/// Simplified cache structure for persisting in filesystem.
type Cache = HashMap<String, FileSystemModificationStatus>;

impl<P: Platform> Daemon<P> {
    /// Creates a clean Daemon instance without reading from the cache.
    /// Performs a health check before returning, returning [Err] if the check fails and [Ok]
    /// otherwise.
    pub fn new(platform: P) -> Result<Self> {
        wrap_err!(HealthCheckError, platform.health_check(), Box::new)?;

        Ok(Daemon {
            platform,
            filesystem_states: RwLock::new(HashMap::new()),
            filesystem_futures: FuturesUnordered::new(),
        })
    }

    fn get_cache_filepath(platform: &P) -> PathBuf {
        platform.get_cache_directory().join("cache.msgpack")
    }

    /// Creates a Daemon instance after reading current values from the cache.
    /// Performs a health check before returning, returning [Err] if the check fails.
    /// Also returns [Err] if the cache is in an invalid state.
    pub fn from_cache(platform: P) -> Result<Self> {
        // now, let's read the cache
        let cache_contents = std::fs::read(Self::get_cache_filepath(&platform));

        let mut daemon = Self::new(platform)?;

        if let Ok(cache_contents) = cache_contents {
            let deserialized_cache: Cache =
                wrap_err!(CacheParse, rmp_serde::decode::from_slice(&cache_contents))?;

            let processed_map = deserialized_cache
                .into_iter()
                .map(|(key, val)| {
                    (
                        key,
                        Mutex::new(FileSystemState {
                            path: None,
                            status: val,
                            has_active_watcher: false,
                            is_daemon_monitoring: false,
                        }),
                    )
                })
                .collect();

            daemon.filesystem_states = RwLock::new(processed_map);
        }

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

        let encoded_cache =
            rmp_serde::encode::to_vec(&normalized_cache).expect("Cache encoding shouldn't fail");

        let cache_directory = self.platform.get_cache_directory();
        // first, create cache path directory
        wrap_err!(CacheError, fs::create_dir_all(&cache_directory).await)?;

        let cache_filepath = Self::get_cache_filepath(&self.platform);
        // finally, write it to the filesystem
        wrap_err!(CacheError, fs::write(cache_filepath, encoded_cache).await)?;

        Ok(())
    }

    async fn mark_filesystem_status(
        &mut self,
        fs_id: &FileSystemID,
        new_status: FileSystemModificationStatus,
    ) -> Result<()> {
        let read_guard = self.filesystem_states.read().await;
        let mut fs_write_guard = {
            let maybe_mutex = read_guard.get(fs_id);

            match maybe_mutex {
                Some(mutex) => mutex.lock().await,
                None => return Err(FSSentinelError::InvalidFileSystemID(fs_id.clone())),
            }
        };

        fs_write_guard.status = new_status.clone();

        use FileSystemModificationStatus::*;
        match new_status {
            Modified => {
                // set active watcher off
                fs_write_guard.has_active_watcher = false;
            }
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
                    .await?;
                }
            }
        }

        Ok(())
    }

    /// Given an ID for a filesystem, marks it as UnModified, prompting the daemon to restart the
    /// corresponding filesystem watcher.
    pub async fn mark_filesystem_unmodified(&mut self, fs_id: &FileSystemID) -> Result<()> {
        // the reason this is exposed instead of the actual `mark_filesystem_status` method is
        // because we don't want users to be able to manually set the modification flag, only unset
        // it
        self.mark_filesystem_status(fs_id, FileSystemModificationStatus::UnModified)
            .await
    }

    /// Given an ID for a filesystem, yield the current status of the corresponding filesystem.
    /// Yields an error if the filesystem ID is invalid.
    pub async fn get_filesystem_status(
        &self,
        fs_id: &FileSystemID,
    ) -> Result<FileSystemModificationStatus> {
        let read_guard = self.filesystem_states.read().await;
        match read_guard.get(fs_id) {
            Some(state) => {
                let guard = state.lock().await;
                Ok(guard.status.clone())
            }
            None => Err(FSSentinelError::InvalidFileSystemID(fs_id.clone())),
        }
    }

    // TODO: don't require an owned `FileSystem`
    async fn start_monitoring_filesystem(
        &mut self,
        fs: FileSystem,
    ) -> std::result::Result<(), FSSentinelError> {
        // if a filesystem is not present in our `statuses`, we will initialize it to `modified`
        let statuses_ro = self.filesystem_states.read().await;
        let filesystem_exists = statuses_ro.contains_key(&fs.id);

        if !filesystem_exists {
            let mut statuses_mutable = self.filesystem_states.write().await;

            statuses_mutable.insert(
                fs.id.clone(),
                Mutex::new(FileSystemState {
                    path: Some(fs.path.clone()),
                    status: FileSystemModificationStatus::UnModified,
                    has_active_watcher: true,
                    is_daemon_monitoring: true,
                }),
            );
        } else {
            let mut state = statuses_ro
                .get(&fs.id)
                .expect("Must exist since we just checked")
                .lock()
                .await;

            // there's no point rewatching it!
            if matches!(state.status, FileSystemModificationStatus::Modified) {
                return Ok(());
            }

            state.has_active_watcher = true;
        }

        let watcher = wrap_err!(
            FileSystemWatchError,
            self.platform.get_filesystem_watcher(fs),
            Box::new
        )?;
        let future = watcher.wait_for_change();
        self.filesystem_futures.push(future);

        Ok(())
    }

    async fn process_ipc_input(&mut self, input: IPCInput) -> Result<IPCOutput> {
        use IPCInput::*;
        use IPCOutput::*;

        match input {
            GetFileSystemStatus(id) => {
                let status = self.get_filesystem_status(&id).await?;
                Ok(FileSystemStatus(status))
            }
            MarkFileSystemUnModified(id) => {
                self.mark_filesystem_unmodified(&id).await?;
                Ok(Success)
            }
            ListAllModifiedFileSystems => {
                let read_guard = self.filesystem_states.read().await;
                // futures::stream::iter converts an iterator into an async Stream; Stream methods
                // take async closures.
                let filesystems = futures::stream::iter(read_guard.iter())
                    .filter_map(|(id, state)| async move {
                        let inner = state.lock().await;
                        if inner.is_daemon_monitoring
                            && matches!(inner.status, FileSystemModificationStatus::Modified)
                        {
                            Some(id.clone())
                        } else {
                            None
                        }
                    })
                    .collect()
                    .await;

                Ok(FileSystemList(filesystems))
            }
        }
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
                    let state = state.get_mut();
                    state.path = Some(fs.path.clone());
                    state.is_daemon_monitoring = true;
                })
                .or_insert_with(|| {
                    Mutex::new(FileSystemState {
                        path: Some(fs.path.clone()),
                        status: FileSystemModificationStatus::UnModified,
                        has_active_watcher: false,
                        is_daemon_monitoring: true,
                    })
                });

            // OPTIMIZE: do asynchronously
            self.start_monitoring_filesystem(fs).await?;
        }

        // let's open up our IPC socket
        let listener = wrap_err!(SocketError, UnixListener::bind(SOCKET_PATH))?;
        let socket_stream = UnixListenerStream::new(listener);

        let loop_res = self.main_loop(socket_stream).await;

        // delete the socket file, regardless of result;
        let deletion_res = wrap_err!(SocketError, tokio::fs::remove_file(SOCKET_PATH).await);

        // finally, return loop_res's Err OR deletion_res's Err OR Ok(());
        loop_res.and(deletion_res)
    }

    async fn main_loop(&mut self, mut socket_stream: UnixListenerStream) -> Result<()> {
        // need to pin in order to use in select!
        let sigint_signal = ctrl_c();
        // have to have this before `shutdown_signals` declaration so that it can get dropped after
        // it.
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        let mut sigterm =
            signal(SignalKind::terminate()).expect("SIGTERM should work on Linux and MacOS");

        // needed to use Pin<Box> instead of just Box in order to make the whole type properly
        // implement Future
        let mut shutdown_signals: FuturesUnordered<Pin<Box<dyn Future<Output = ()>>>> =
            FuturesUnordered::new();

        shutdown_signals.push(Box::pin(sigint_signal.map(|_| ())));
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        shutdown_signals.push(Box::pin(sigterm.recv().map(|_| ())));

        const BUFFER_SIZE: usize = 1024;
        let mut buffer: Vec<u8> = vec![0; BUFFER_SIZE];

        // now, let's do our main loop
        loop {
            tokio::select! {
                Some(next_fs_id) = self.filesystem_futures.next(), if !self.filesystem_futures.is_empty() => {
                    // now, mark the filesystem as modified
                    self.mark_filesystem_status(&next_fs_id, FileSystemModificationStatus::Modified)
                        .await?;
                },
                Some(stream) = socket_stream.next() => {
                    let mut stream = wrap_err!(SocketError, stream)?;
                    // NOTE: stream.{read,write} come from Async{Read,Write}Ext, which is in the io-util
                    // feature of tokio
                    let num_bytes = wrap_err!(SocketError, stream.read(&mut buffer).await)?;

                    // parse it into structure
                    let input: IPCInput = wrap_err!(MessageParse, rmp_serde::decode::from_slice(&buffer[..num_bytes]))?;

                    // now, process accordingly
                    let ipc_response = match self.process_ipc_input(input).await {
                        Ok(response) => response,
                        Err(e) => IPCOutput::DaemonError(e.to_string())
                    };

                    let encoded_response = rmp_serde::encode::to_vec(&ipc_response).expect("Encoding shouldn't fail");

                    // write the response
                    wrap_err!(SocketError, stream.write_all(&encoded_response).await)?;

                    wrap_err!(SocketError, stream.shutdown().await)?;
                }
                _ = shutdown_signals.next() => {
                    eprintln!("Shutting down!");
                    self.shutdown().await?;
                    break
                }
            }
        }

        Ok(())
    }

    async fn shutdown(&mut self) -> Result<()> {
        self.update_cache().await
    }
}
