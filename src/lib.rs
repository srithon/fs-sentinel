use futures::stream::FuturesUnordered;
use std::collections::HashMap;
use tokio::sync::{Mutex, RwLock};

mod platform;

use platform::Platform;

#[cfg(feature = "linux")]
mod linux;

pub type FileSystemID = String;

pub enum FileSystemStatus {
    Modified,
    UnModified
}

/// State maps filesystems to whether or not they were modified.
pub struct Daemon<P: Platform> {
    platform: P,
    filesystem_futures: FuturesUnordered<FileSystemID>,
    filesystem_statuses: RwLock<HashMap<FileSystemID, Mutex<FileSystemStatus>>>,
/// A FileSystem combines a FileSystemID with a path contained within the FileSystem; for any
/// requirements concerning the path itself, see the documentation for your used `Platform`.
pub struct FileSystem {
    pub id: FileSystemID,
    pub path: PathBuf
}

}
