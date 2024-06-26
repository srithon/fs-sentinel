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
    CacheError {
        #[source]
        source: io::Error,
    },

    #[error("couldn't parse cache")]
    CacheParse {
        #[source]
        source: rmp_serde::decode::Error,
    },

    #[error("couldn't parse ipc message")]
    MessageParse {
        #[source]
        source: rmp_serde::decode::Error,
    },

    #[error("invalid filesystem id: {0:#?}")]
    InvalidFileSystemID(FileSystemID),

    #[error("error while reading/writing socket")]
    SocketError {
        #[source]
        source: io::Error,
    },

    #[error("error during platform health check")]
    HealthCheckError {
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    #[error("error while trying to watch filesystem")]
    // see [Platform] documentation for reasoning behind `Send + Sync` bound
    FileSystemWatchError {
        source: Box<dyn std::error::Error + Send + Sync>,
    },
}

macro_rules! wrap_err {
    ($variant:ident, $result:expr) => {{
        wrap_err!($variant, $result, std::convert::identity)
    }};
    ($variant:ident, $result:expr, $map_fn:expr) => {{
        $result.map_err(|error| FSSentinelError::$variant { source: $map_fn(error) })
    }};
}

pub(crate) use wrap_err;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum FileSystemModificationStatus {
    Modified,
    UnModified,
}

pub type Result<T> = std::result::Result<T, FSSentinelError>;

/// A FileSystem combines a FileSystemID with a path contained within the FileSystem; for any
/// requirements concerning the path itself, see the documentation for your used `Platform`.
#[derive(Clone, Debug)]
pub struct FileSystem {
    pub id: FileSystemID,
    pub path: PathBuf,
}
