//! This module implements `Platform` for Linux, using the `fsnotifywait` command-line tool to
//! watch for events, which uses the modern `fanotify` API under the hood. This approach was chosen
//! because unlike with `inotify`, as of this writing, there is currently no widely-used crate
//! binding for `fanotify`.
use async_trait::async_trait;
use std::path::PathBuf;
use std::process::Command as SyncCommand;
use tokio::process::Command;

use crate::{
    platform::{FileSystemWatcher, Platform},
    FileSystem, FileSystemID,
};

use thiserror::Error;

/// Platform-specific implementation for Linux.
pub struct Linux;

#[derive(Error, Debug)]
pub enum Error {
    #[error("can't find fsnotifywait binary in PATH; see README for more details")]
    MissingFSNotifyWait,
    #[error("filesystem path doesn't exist or isn't accessible")]
    InvalidFileSystemPath(PathBuf),
}

impl Platform for Linux {
    type Watcher = FSNotifyWaitWatcher;
    type Error = Error;

    fn get_cache_directory(&self) -> PathBuf {
        "/var/cache/fs-sentinel".into()
    }

    /// For the Linux platform, `filesystem.path` can be ANY directory within the Filesystem.
    fn get_filesystem_watcher(&self, filesystem: FileSystem) -> Result<Self::Watcher, Self::Error> {
        if !filesystem.path.exists() {
            return Err(Error::InvalidFileSystemPath(filesystem.path));
        }

        Ok(FSNotifyWaitWatcher(filesystem))
    }

    /// Verifies that fsnotifywait is installed.
    fn health_check(&self) -> Result<(), Self::Error> {
        let fsnotifywait_handle = SyncCommand::new("fsnotifywait").spawn();
        match fsnotifywait_handle {
            Ok(mut handle) => {
                // reap child process
                let _ = handle.wait();
            }
            Err(_) => return Err(Error::MissingFSNotifyWait),
        };

        Ok(())
    }
}

/// `FileSystemWatcher` implementation which uses the `fsnotifywait` command-line tool.
pub struct FSNotifyWaitWatcher(FileSystem);

#[async_trait]
impl FileSystemWatcher for FSNotifyWaitWatcher {
    async fn wait_for_change(self) -> FileSystemID {
        // first, let's create the command
        let mut command = Command::new("fsnotifywait");
        // we only want the output from `--format`.
        command.arg("--quiet");
        // watch the entire filesystem associated with the directory.
        command.arg("--filesystem");
        // only watch the following events, which we consider to be important modifications
        command.arg("--event");
        command.arg(
            "modify,attrib,close_write,moved_to,moved_from,move_self,create,delete,delete_self",
        );

        // let's format the output so that we get a comma-separated list of the relevant events.
        // command.arg("--format");
        // command.arg("%e");

        // finally, the directory
        command.arg(self.0.path);

        // now, let's run the command, waiting for it to complete.
        let _ = command
            .spawn()
            .expect("Command should execute normally.")
            .wait()
            .await;

        // then, let's yield the filesystem id
        self.0.id
    }
}
