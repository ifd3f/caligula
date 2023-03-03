use interprocess::local_socket::tokio::LocalSocketListener;
use interprocess::local_socket::tokio::LocalSocketStream;
use std::{env, pin::Pin, process::Stdio};
use tokio::io::AsyncBufReadExt;
use tokio::io::BufReader;
use tokio_util::compat::FuturesAsyncReadCompatExt;
use tokio_util::compat::FuturesAsyncWriteCompatExt;
use tracing::debug;
use uuid::Uuid;
use valuable::Valuable;

use tokio::{
    fs,
    io::{AsyncBufRead, AsyncWrite},
    process::{Child, Command},
};

use super::{
    ipc::{BurnConfig, StatusMessage, TerminateResult},
    BURN_ENV,
};

pub struct Handle {
    child: Child,
    socket: LocalSocketListener,
    rx: Pin<Box<dyn AsyncBufRead>>,
    tx: Pin<Box<dyn AsyncWrite>>,
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

        let socket_name = env::temp_dir().join(format!("caligula-{}.pipe", Uuid::new_v4()));
        debug!(
            socket_name = format!("{}", socket_name.to_string_lossy()),
            "Creating socket"
        );
        let mut socket = LocalSocketListener::bind(socket_name.clone())?;

        let mut cmd = (if escalate {
            let mut cmd = Command::new("sudo");
            cmd.arg(format!("{BURN_ENV}=1")).arg(proc);
            cmd
        } else {
            let mut cmd = Command::new(&proc);
            cmd.env(BURN_ENV, "1");
            cmd
        });
        cmd.arg(socket_name).arg(args).kill_on_drop(true);

        debug!(cmd = format!("{:?}", cmd), "Starting child process");
        let child = cmd.spawn()?;

        debug!("Waiting for pipe to be opened...");
        let stream: LocalSocketStream = socket.accept().await?;
        let (rx, tx) = stream.into_split();

        let mut proc = Self {
            child,
            socket,
            rx: Box::pin(BufReader::new(rx.compat())),
            tx: Box::pin(tx.compat_write()),
        };

        debug!("Reading results from stdout");
        let first_msg = proc.next_message().await?;
        debug!(
            first_msg = first_msg.as_value(),
            "Read raw result from stdout"
        );

        match first_msg {
            Some(StatusMessage::FileOpenSuccess) => Ok(proc),
            Some(StatusMessage::Terminate(t)) => Err(StartProcessError::Failed(Some(t)))?,
            Some(other) => Err(StartProcessError::UnexpectedFirstStatus(other))?,
            None => Err(StartProcessError::UnexpectedEOF)?,
        }
    }

    pub async fn next_message(&mut self) -> anyhow::Result<Option<StatusMessage>> {
        let mut line = String::new();
        let count = self.rx.read_line(&mut line).await?;
        if count == 0 {
            return Ok(None);
        }

        debug!(line, "Got line");

        let message = serde_json::from_str::<StatusMessage>(&line)?;
        debug!(message = format!("{message:?}"), "Parsed message");

        Ok(Some(message))
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
