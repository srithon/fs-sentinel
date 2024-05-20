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
    #[structopt(about = "Runs the daemon with a specified list of monitored filesystems")]
    Daemon {
        #[structopt(parse(try_from_str = try_parse_filesystem), help = "List of file systems, each in 'id=path' format, and each passed as a separate command-line argument. Note that each `id` can be an arbitrary string identifier, so long as all ids are unique. Also, the `path`s must all exist.", required = true)]
        filesystems: Vec<FileSystem>,
    },
    #[structopt(about = "Resets the specified filesystem to unmodified")]
    Mark {
        #[structopt(
            help = "This filesystem id must correspond to an id component passed into the daemon initialization"
        )]
        filesystem_id: String,
    },
    #[structopt(
        about = "If the specified filesystem has been modified, returns 0, otherwise returns 1"
    )]
    Check {
        #[structopt(
            help = "This filesystem id must correspond to an id component passed into the daemon initialization"
        )]
        filesystem_id: String,
    },
    #[structopt(
        about = "Yields a newline-delimited list of filesystem ids corresponding to all modified filesystems."
    )]
    ListModified,
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
                ListModified => IPCInput::ListAllModifiedFileSystems,
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
                IPCOutput::FileSystemList(filesystem_ids) => {
                    println!("{}", filesystem_ids.join("\n"));
                    ExitCode::SUCCESS
                }
            }
        }
    };

    Ok(exit_code)
}
