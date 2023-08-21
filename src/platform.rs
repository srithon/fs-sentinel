use async_trait::async_trait;
use std::path::PathBuf;

use crate::FileSystemID;

/// Abstraction for all platform-specific behavior.
pub trait Platform
where
    Self::Watcher: FileSystemWatcher,
{
    /// Platform-specific FileSystemWatcher type, used in the return value for `get_filesystem_watcher`.
    type Watcher;

    /// Yields the directory for the cache file to be stored on the given platform.
    fn get_cache_path() -> PathBuf;

    /// Given a path, yields an instance of `Watcher`, which will monitor the filesystem containing
    /// `path`.
    fn get_filesystem_watcher(
        filesystem_identifier: FileSystemID,
        filesystem_path: PathBuf,
    ) -> Self::Watcher;
}

/// Abstraction for watching a single file system for changes.
#[async_trait]
pub trait FileSystemWatcher {
    /// Function that terminates when there is a change on the filesystem, yielding the
    /// filesystem's identifier.
    async fn wait_for_change(self) -> FileSystemID;
}
