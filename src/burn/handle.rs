use interprocess::local_socket::tokio::LocalSocketListener;
use interprocess::local_socket::tokio::LocalSocketStream;
use rand::distributions::Alphanumeric;
use rand::distributions::DistString;
use std::fs::remove_file;
use std::path::PathBuf;
use std::{env, pin::Pin};
use tokio::io::AsyncBufReadExt;
use tokio::io::BufReader;
use tokio_util::compat::FuturesAsyncReadCompatExt;
use tokio_util::compat::FuturesAsyncWriteCompatExt;
use tracing::debug;
use tracing::trace;
use tracing_unwrap::ResultExt;
use valuable::Valuable;

use tokio::{
    fs,
    io::{AsyncBufRead, AsyncWrite},
    process::{Child, Command},
};

use super::ipc::InitialInfo;
use super::{
    ipc::{BurnConfig, StatusMessage, TerminateResult},
    BURN_ENV,
};

pub struct Handle {
    _child: Child,
    _socket: ChildSocket,
    initial_info: InitialInfo,
    rx: Pin<Box<dyn AsyncBufRead>>,
    _tx: Pin<Box<dyn AsyncWrite>>,
}

impl Handle {
    pub async fn start(args: &BurnConfig, escalate: bool) -> anyhow::Result<Self> {
        // Get path to this process
        let proc = fs::read_link("/proc/self/exe").await?;

        debug!(
            proc = proc.to_string_lossy().to_string(),
            "Read absolute path to this program"
        );

        let args = serde_json::to_string(args)?;
        debug!("Converted BurnConfig to JSON: {args}");

        let mut socket = ChildSocket::new()?;

        let mut cmd = if escalate {
            let mut cmd = Command::new("sudo");
            cmd.arg(format!("{BURN_ENV}=1")).arg(proc);
            cmd
        } else {
            let mut cmd = Command::new(&proc);
            cmd.env(BURN_ENV, "1");
            cmd
        };
        cmd.arg(args).arg(&socket.socket_name).kill_on_drop(true);

        debug!("Starting child process with command: {:?}", cmd.as_std());
        let child = cmd.spawn()?;

        debug!("Waiting for pipe to be opened...");
        let stream: LocalSocketStream = socket.accept().await?;
        let (rx, tx) = stream.into_split();
        let mut rx = Box::pin(BufReader::new(rx.compat()));
        let tx = Box::pin(tx.compat_write());

        trace!("Reading results from child");
        let first_msg = read_next_message(&mut rx).await?;
        debug!(
            first_msg = first_msg.as_value(),
            "Read raw result from child"
        );

        let initial_info = match first_msg {
            Some(StatusMessage::InitSuccess(i)) => Ok(i),
            Some(StatusMessage::Terminate(t)) => Err(StartProcessError::Failed(Some(t))),
            Some(other) => Err(StartProcessError::UnexpectedFirstStatus(other)),
            None => Err(StartProcessError::UnexpectedEOF),
        }?;

        Ok(Self {
            _child: child,
            _socket: socket,
            initial_info,
            rx,
            _tx: tx,
        })
    }

    pub async fn next_message(&mut self) -> anyhow::Result<Option<StatusMessage>> {
        read_next_message(&mut self.rx).await
    }

    pub fn initial_info(&self) -> &InitialInfo {
        &self.initial_info
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum StartProcessError {
    #[error("Unexpected first status: {0:?}")]
    UnexpectedFirstStatus(StatusMessage),
    #[error("Unexpected end of stdout")]
    UnexpectedEOF,
    #[error("Explicit failure signaled: {0:?}")]
    Failed(Option<TerminateResult>),
}

async fn read_next_message(
    mut rx: impl AsyncBufRead + Unpin,
) -> anyhow::Result<Option<StatusMessage>> {
    let mut line = String::new();
    let count = rx.read_line(&mut line).await?;
    if count == 0 {
        return Ok(None);
    }
    trace!(line, "Got line from child process");

    let message = serde_json::from_str::<StatusMessage>(&line)?;
    trace!(message = format!("{message:?}"), "Parsed message");

    Ok(Some(message))
}

/// A managed named socket. It gets auto-deleted on drop.
struct ChildSocket {
    socket_name: PathBuf,
    socket: LocalSocketListener,
}

impl ChildSocket {
    fn new() -> anyhow::Result<Self> {
        let socket_name: PathBuf = env::temp_dir().join(format!(
            "caligula-{}.sock",
            Alphanumeric.sample_string(&mut rand::thread_rng(), 32)
        ));
        debug!(
            socket_name = format!("{}", socket_name.to_string_lossy()),
            "Creating socket"
        );
        let socket = LocalSocketListener::bind(socket_name.clone())?;

        Ok(Self {
            socket,
            socket_name,
        })
    }

    async fn accept(&mut self) -> anyhow::Result<LocalSocketStream> {
        Ok(self.socket.accept().await?)
    }
}

impl Drop for ChildSocket {
    fn drop(&mut self) {
        remove_file(&self.socket_name).unwrap_or_log();
    }
}
