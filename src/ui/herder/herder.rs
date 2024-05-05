use crate::ui::herder::handle::ChildHandle;
use crate::{
    ipc_common::read_msg_async, logging::get_log_paths, run_mode::make_writer_spawn_command,
    ui::herder::socket::HerderSocket, writer_process::ipc::ErrorType,
};
use anyhow::Context;
use interprocess::local_socket::tokio::prelude::*;
use process_path::get_executable_path;
use tracing::{debug, trace};
use valuable::Valuable;

use crate::escalation::run_escalate;
use crate::writer_process::ipc::{StatusMessage, WriterProcessConfig};

use super::handle::WriterHandle;

/// Handles the herding of all child processes. This includes lifecycle management
/// and communication.
///
/// Why "Herder"? It's a horse, and horses are herded. I think. I'm not a farmer.
pub struct Herder {
    socket: HerderSocket,
}

impl Herder {
    pub fn new(socket: HerderSocket) -> Self {
        Self { socket }
    }

    pub async fn start_writer(
        &mut self,
        args: &WriterProcessConfig,
        escalate: bool,
    ) -> anyhow::Result<WriterHandle> {
        // Get path to this process
        let proc = get_executable_path().unwrap();

        debug!(
            proc = proc.to_string_lossy().to_string(),
            "Read absolute path to this program"
        );

        let log_path = get_log_paths().writer(0);
        let cmd = make_writer_spawn_command(
            self.socket.socket_name().to_string_lossy(),
            log_path.to_string_lossy(),
            args,
        );

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
        let stream: LocalSocketStream = self.socket.accept().await?;
        let mut handle = ChildHandle::new(Some(child), stream);

        trace!("Reading results from child");
        let first_msg = read_msg_async::<StatusMessage>(&mut handle.rx).await?;
        debug!(
            first_msg = first_msg.as_value(),
            "Read raw result from child"
        );

        let initial_info = match first_msg {
            StatusMessage::InitSuccess(i) => Ok(i),
            StatusMessage::Error(t) => Err(StartWriterError::Failed(Some(t))),
            other => Err(StartWriterError::UnexpectedFirstStatus(other)),
        }?;

        Ok(WriterHandle {
            handle,
            initial_info,
        })
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum StartWriterError {
    #[error("Unexpected first status: {0:?}")]
    UnexpectedFirstStatus(StatusMessage),
    #[error("Explicit failure signaled: {0:?}")]
    Failed(Option<ErrorType>),
}
