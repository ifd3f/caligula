use std::sync::Arc;

use crate::escalated_daemon::ipc::{EscalatedDaemonInitConfig, SpawnWriter};
use crate::ipc_common::write_msg_async;
use crate::logging::LogPaths;
use crate::run_mode::make_escalated_daemon_spawn_command;
use crate::ui::herder::handle::ChildHandle;
use crate::{
    ipc_common::read_msg_async, run_mode::make_writer_spawn_command,
    ui::herder::socket::HerderSocket, writer_process::ipc::ErrorType,
};
use anyhow::Context;
use interprocess::local_socket::tokio::prelude::*;
use tracing::{debug, trace};

use crate::escalation::run_escalate;
use crate::writer_process::ipc::{StatusMessage, WriterProcessConfig};

use super::handle::WriterHandle;

/// Handles the herding of all child processes. This includes lifecycle management
/// and communication.
///
/// Why "Herder"? Caligula liked his horse, and horses are herded. I think. I'm not
/// a farmer.
pub struct Herder {
    socket: HerderSocket,
    log_paths: Arc<LogPaths>,
    escalated_daemon: Option<ChildHandle>,
    next_writer_id: u64,
}

impl Herder {
    pub fn new(socket: HerderSocket, log_paths: Arc<LogPaths>) -> Self {
        Self {
            socket,
            escalated_daemon: None,
            log_paths,
            next_writer_id: 0,
        }
    }

    #[tracing::instrument(skip_all)]
    async fn ensure_escalated_daemon(&mut self) -> anyhow::Result<&mut ChildHandle> {
        // Can't use if let here because of polonius! so we gotta do this ugly-ass workaround
        if self.escalated_daemon.is_none() {
            let log_path = self.log_paths.escalated_daemon();
            let cmd = make_escalated_daemon_spawn_command(
                self.socket.socket_name().to_string_lossy(),
                log_path.to_string_lossy(),
                &EscalatedDaemonInitConfig {},
            );

            debug!("Starting child process with command: {:?}", cmd);
            fn modify_cmd(cmd: &mut tokio::process::Command) {
                cmd.kill_on_drop(true);
            }
            let child = run_escalate(&cmd, modify_cmd)
                .await
                .context("Failed to spawn escalated daemon process")?;

            debug!(?child, "Process spawned, waiting for pipe to be opened...");
            let stream: LocalSocketStream = self.socket.accept().await?;
            let handle = ChildHandle::new(Some(child), stream);

            self.escalated_daemon = Some(handle);
        }

        Ok(self.escalated_daemon.as_mut().unwrap())
    }

    #[tracing::instrument(skip_all, fields(escalate))]
    pub async fn start_writer(
        &mut self,
        args: &WriterProcessConfig,
        escalate: bool,
    ) -> anyhow::Result<WriterHandle> {
        let log_path = self.log_paths.writer(self.next_writer_id);
        self.next_writer_id += 1;

        let child = if escalate {
            let daemon = self.ensure_escalated_daemon().await?;
            write_msg_async(
                &mut daemon.tx,
                &SpawnWriter {
                    log_file: log_path.to_string_lossy().to_string(),
                    init_config: args.clone(),
                },
            )
            .await?;
            None
        } else {
            let cmd = make_writer_spawn_command(
                self.socket.socket_name().to_string_lossy(),
                log_path.to_string_lossy(),
                args,
            );
            debug!("Directly spawning child process with command: {:?}", cmd);

            let mut cmd = tokio::process::Command::from(cmd);
            cmd.kill_on_drop(true);
            Some(cmd.spawn().context("Failed to spawn child process")?)
        };

        debug!("Waiting for pipe to be opened...");
        let stream: LocalSocketStream = self.socket.accept().await?;
        let mut handle = ChildHandle::new(child, stream);

        trace!("Reading results from child");
        let first_msg = read_msg_async::<StatusMessage>(&mut handle.rx).await?;
        debug!(?first_msg, "Read raw result from child");

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
