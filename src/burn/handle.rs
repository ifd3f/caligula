use process_path::get_executable_path;
use std::pin::Pin;
use std::process::Stdio;
use tokio::io::BufReader;
use tracing::debug;
use tracing::trace;
use valuable::Valuable;

use tokio::{
    io::{AsyncBufRead, AsyncWrite},
    process::Child,
};

use crate::burn::ipc::read_msg_async;
use crate::escalation::run_escalate;
use crate::escalation::Command;

use super::ipc::InitialInfo;
use super::{
    ipc::{BurnConfig, ErrorType, StatusMessage},
    BURN_ENV,
};

pub struct Handle {
    _child: Child,
    initial_info: InitialInfo,
    rx: Pin<Box<dyn AsyncBufRead>>,
    _tx: Pin<Box<dyn AsyncWrite>>,
}

impl Handle {
    pub async fn start(args: &BurnConfig, escalate: bool) -> anyhow::Result<Self> {
        // Get path to this process
        let proc = get_executable_path().unwrap();

        debug!(
            proc = proc.to_string_lossy().to_string(),
            "Read absolute path to this program"
        );

        let args = serde_json::to_string(args)?;
        debug!("Converted BurnConfig to JSON: {args}");

        let cmd = Command {
            proc: proc.to_string_lossy(),
            envs: vec![(BURN_ENV.into(), "1".into())],
            args: vec![args.into()],
        };

        debug!("Starting child process with command: {:?}", cmd);
        let mut child = if escalate {
            run_escalate(&cmd).await?
        } else {
            tokio::process::Command::from(cmd)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .kill_on_drop(true)
                .spawn()?
        };

        debug!("Waiting for pipe to be opened...");
        let mut rx = Box::pin(BufReader::new(
            child
                .stdout
                .take()
                .expect("Failed to get stdout of child process"),
        ));
        let tx = Box::pin(
            child
                .stdin
                .take()
                .expect("Failed to get stdin of child process"),
        );

        trace!("Reading results from child");
        let first_msg = read_next_message(&mut rx).await?;
        debug!(
            first_msg = first_msg.as_value(),
            "Read raw result from child"
        );

        let initial_info = match first_msg {
            Some(StatusMessage::InitSuccess(i)) => Ok(i),
            Some(StatusMessage::Error(t)) => Err(StartProcessError::Failed(Some(t))),
            Some(other) => Err(StartProcessError::UnexpectedFirstStatus(other)),
            None => Err(StartProcessError::UnexpectedEOF),
        }?;

        Ok(Self {
            _child: child,
            initial_info,
            rx,
            _tx: tx,
        })
    }

    pub async fn next_message(&mut self) -> std::io::Result<Option<StatusMessage>> {
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
    Failed(Option<ErrorType>),
}

async fn read_next_message(
    rx: impl AsyncBufRead + Unpin,
) -> std::io::Result<Option<StatusMessage>> {
    Ok(Some(read_msg_async(rx).await?))
}

impl core::fmt::Debug for Handle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Handle")
            .field("_child", &self._child)
            .field("initial_info", &self.initial_info)
            .finish()
    }
}
