use crate::{ipc_common::read_msg_async, ui::herder::socket::HerderSocket};
use anyhow::Context;
use interprocess::local_socket::tokio::prelude::*;
use process_path::get_executable_path;
use std::pin::Pin;
use tokio::io::BufReader;
use tracing::{debug, trace};
use valuable::Valuable;

use tokio::{
    io::{AsyncBufRead, AsyncWrite},
    process::Child,
};

use crate::escalation::run_escalate;
use crate::escalation::Command;
use crate::run_mode::RunMode;
use crate::run_mode::RUN_MODE_ENV_NAME;
use crate::writer_process::ipc::{ErrorType, InitialInfo, StatusMessage, WriterProcessConfig};

pub struct WriterHandle {
    _child: Child,
    _socket: HerderSocket,
    initial_info: InitialInfo,
    rx: Pin<Box<dyn AsyncBufRead>>,
    _tx: Pin<Box<dyn AsyncWrite>>,
}

impl WriterHandle {
    pub async fn start(args: &WriterProcessConfig, escalate: bool) -> anyhow::Result<Self> {
        // Get path to this process
        let proc = get_executable_path().unwrap();

        debug!(
            proc = proc.to_string_lossy().to_string(),
            "Read absolute path to this program"
        );

        let args = serde_json::to_string(args)?;
        debug!(?args, "Converted WriterProcessConfig to JSON");

        let mut socket = HerderSocket::new().await?;

        let cmd = Command {
            proc: proc.to_string_lossy(),
            envs: vec![(RUN_MODE_ENV_NAME.into(), RunMode::Writer.as_str().into())],
            args: vec![args.into(), socket.socket_name().to_string_lossy().into()],
        };

        debug!("Starting child process with command: {:?}", cmd);
        fn modify_cmd(cmd: &mut tokio::process::Command) {
            cmd.kill_on_drop(true);
        }
        let child = if escalate {
            run_escalate(&cmd, modify_cmd)
                .await
                .context("Failed to spawn child process")?
        } else {
            let mut cmd = tokio::process::Command::from(cmd);
            modify_cmd(&mut cmd);
            cmd.spawn().context("Failed to spawn child process")?
        };

        debug!("Waiting for pipe to be opened...");
        let stream: LocalSocketStream = socket.accept().await?;
        let (rx, tx) = stream.split();
        let mut rx = Box::pin(BufReader::new(rx));
        let tx = Box::pin(tx);

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
    Failed(Option<ErrorType>),
}

async fn read_next_message(rx: impl AsyncBufRead + Unpin) -> anyhow::Result<Option<StatusMessage>> {
    let message = read_msg_async(rx).await?;
    Ok(Some(message))
}

impl core::fmt::Debug for WriterHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Handle")
            .field("_child", &self._child)
            .field("_socket", &self._socket)
            .field("initial_info", &self.initial_info)
            .finish()
    }
}
