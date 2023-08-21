use futures::stream::FuturesUnordered;
use std::collections::HashMap;
use tokio::sync::{Mutex, RwLock};

mod platform;

use platform::Platform;

#[cfg(feature = "linux")]
mod linux;

pub type FileSystemID = String;

/// State maps filesystems to whether or not they were modified.
pub struct Daemon<P: Platform> {
    platform: P,
    filesystem_statuses: RwLock<HashMap<FileSystemID, Mutex<bool>>>,
    filesystem_futures: FuturesUnordered<FileSystemID>,
}
