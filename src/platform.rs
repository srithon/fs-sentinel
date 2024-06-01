use async_trait::async_trait;
use std::path::PathBuf;

use crate::{FileSystem, FileSystemID};

/// Abstraction for all platform-specific behavior.
pub trait Platform
where
    Self::Watcher: FileSystemWatcher,
{
    /// Platform-specific FileSystemWatcher type, used in the return value for `get_filesystem_watcher`.
    type Watcher: 'static;

    /// Platform-specific error type.
    /// The `Send + Sync` restriction is unfortunately a violation of the "Dependency Inversion
    /// Principle".
    /// 1. In order to properly abstract over different `Platform`'s, we need to impose a
    ///    bound on all `Platform` `Error` types
    /// 2. Because crates like `anyhow` require that errors are `Send + Sync`, and there is no good
    ///    way to "wrap" a `dyn Error` and make it `Send + Sync` after stripping away its
    ///    identifying information, we must introduce this trait bound at the very source. However,
    ///    to be fair, it should be noted that `std::io::Error` also requires that all of its
    ///    wrapped errors implement `Send + Sync`, so this is not an unreasonable request.
    type Error: std::error::Error + Send + Sync + 'static;

    /// Yields the directory for the cache file to be stored on the given platform.
    fn get_cache_directory(&self) -> PathBuf;

    /// Given a path, yields an instance of `Watcher`, which will monitor the filesystem containing
    /// `path`.
    fn get_filesystem_watcher(&self, filesystem: FileSystem) -> Self::Watcher;

    /// Returns `Ok(())` if the underlying system supports the given `Platform`, otherwise returning
    /// `Err(E)`.
    fn health_check(&self) -> Result<(), Self::Error>;
}

/// Abstraction for watching a single file system for changes.
#[async_trait]
pub trait FileSystemWatcher {
    /// Function that terminates when there is a change on the filesystem, yielding the
    /// FileSystem's identifier.
    async fn wait_for_change(self) -> FileSystemID;
}
