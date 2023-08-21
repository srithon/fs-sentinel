//! This module implements `Platform` for Linux, using the `fsnotifywait` command-line tool to
//! watch for events, which uses the modern `fanotify` API under the hood. This approach was chosen
//! because unlike with `inotify`, as of this writing, there is currently no widely-used crate
//! binding for `fanotify`.
use async_trait::async_trait;
use std::path::PathBuf;
use tokio::process::Command;

use crate::{
    platform::{FileSystemWatcher, Platform},
    FileSystemID,
};

/// Platform-specific implementation for Linux.
pub struct Linux;

impl Platform for Linux {
    type Watcher = FSNotifyWaitWatcher;

    fn get_cache_path() -> PathBuf {
        "/var/cache/fs-sentinel".into()
    }

    fn get_filesystem_watcher(
        filesystem_identifier: FileSystemID,
        filesystem_path: PathBuf,
    ) -> Self::Watcher {
        FSNotifyWaitWatcher {
            filesystem_identifier,
            filesystem_path,
        }
    }
}

/// `FileSystemWatcher` implementation which uses the `fsnotifywait` command-line tool.
pub struct FSNotifyWaitWatcher {
    filesystem_identifier: FileSystemID,
    filesystem_path: PathBuf,
}

#[async_trait]
impl FileSystemWatcher for FSNotifyWaitWatcher {
    async fn wait_for_change(self) -> FileSystemID {
        // first, let's create the command
        let mut command = Command::new("fsnotifywait");
        // we only want the output from `--format`.
        command.arg("--quiet");
        // watch the entire filesystem associated with the directory.
        command.arg("--filesystem");

        // let's format the output so that we get a comma-separated list of the relevant events.
        // command.arg("--format");
        // command.arg("%e");

        // finally, the directory
        command.arg(self.filesystem_path);

        // now, let's run the command, waiting for it to complete.
        let _ = command
            .spawn()
            .expect("Command should execute normally.")
            .wait()
            .await;

        // then, let's yield the filesystem id
        self.filesystem_identifier
    }
}
