use serde::{Deserialize, Serialize};
use std::io;
use std::path::PathBuf;
use thiserror::Error;

pub mod daemon;
pub mod platform;

#[cfg(feature = "linux")]
pub mod linux;

pub type FileSystemID = String;

#[derive(Error, Debug)]
pub enum FSSentinelError {
    #[error("couldn't read/write cache")]
    CacheError(#[from] io::Error),

    #[error("couldn't parse cache")]
    CacheParse(#[from] rmp_serde::decode::Error),

    #[error("invalid filesystem id: {0:#?}")]
    InvalidFileSystemID(FileSystemID),
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum FileSystemModificationStatus {
    Modified,
    UnModified,
}

pub type Result<T> = std::result::Result<T, FSSentinelError>;

/// A FileSystem combines a FileSystemID with a path contained within the FileSystem; for any
/// requirements concerning the path itself, see the documentation for your used `Platform`.
#[derive(Clone)]
pub struct FileSystem {
    pub id: FileSystemID,
    pub path: PathBuf,
}
