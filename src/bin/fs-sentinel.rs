use std::path::PathBuf;
use std::process::ExitCode;
use std::str::FromStr;

use structopt::StructOpt;

use fs_sentinel::daemon::{IPCInput, IPCOutput, SOCKET_PATH};
use fs_sentinel::linux::Linux;
use fs_sentinel::{FileSystem, FileSystemModificationStatus};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;

use anyhow::{anyhow, ensure, Result};

fn try_parse_filesystem(s: &str) -> Result<FileSystem> {
    // try to parse it into `id=path`
    let (id, path) = s.split_once('=').ok_or_else(|| {
        anyhow!(
            "filesystem spec '{}' not in expected format; expecting 'id=path'",
            s
        )
    })?;

    let path = PathBuf::from_str(path)?;

    // ensure that path is valid
    ensure!(
        path.exists(),
        "Path '{}' must exist! Otherwise, ensure that user has permissions to read path.",
        path.to_string_lossy()
    );

    Ok(FileSystem {
        id: id.to_owned(),
        path,
    })
}

#[derive(StructOpt, Debug)]
enum CLI {
    Daemon {
        #[structopt(parse(try_from_str = try_parse_filesystem))]
        filesystems: Vec<FileSystem>,
    },
    Mark {
        // #[structopt(parse(from_os_str))]
        filesystem_id: String,
    },
    Check {
        // #[structopt(parse(from_os_str))]
        filesystem_id: String,
    },
}

#[tokio::main]
async fn main() -> Result<ExitCode> {
    let cli = CLI::from_args();

    use CLI::*;
    let exit_code = match cli {
        Daemon { filesystems } => {
            let daemon = fs_sentinel::daemon::Daemon::from_cache(Linux)?;
            daemon.run(filesystems).await?;

            ExitCode::SUCCESS
        }
        _ => {
            // otherwise, client
            let mut socket = match UnixStream::connect(SOCKET_PATH).await {
                Ok(socket) => socket,
                Err(e) => {
                    return Err(anyhow!("couldn't connect to socket: {}", e));
                }
            };

            let input = match cli {
                Mark { filesystem_id } => IPCInput::MarkFileSystemUnModified(filesystem_id),
                Check { filesystem_id } => IPCInput::GetFileSystemStatus(filesystem_id),
                _ => unreachable!("every supported option other than Daemon should go here"),
            };

            // encode it using msgpack
            let encoded_input = rmp_serde::to_vec(&input)?;
            socket.write_all(&encoded_input).await?;

            let mut response_buffer = vec![0; 1024];
            // wait for response
            let response_num_bytes = socket.read(&mut response_buffer).await?;
            response_buffer.truncate(response_num_bytes);

            socket.shutdown().await?;

            // finally, decode response and print it out
            let decoded_response: IPCOutput = rmp_serde::from_slice(&response_buffer)?;
            match decoded_response {
                IPCOutput::Success => ExitCode::SUCCESS,
                IPCOutput::DaemonError(err) => return Err(anyhow!(err)),
                IPCOutput::FileSystemStatus(status) => match status {
                    FileSystemModificationStatus::Modified => ExitCode::SUCCESS,
                    // we choose the "default" response of `FAILURE`; this path shares the same
                    // result as the Error branch
                    FileSystemModificationStatus::UnModified => ExitCode::FAILURE,
                },
            }
        }
    };

    Ok(exit_code)
}
