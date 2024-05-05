use crate::{
    ipc_common::read_msg_async,
    ui::herder::{
        socket::HerderSocket,
        writer::handle::{StartProcessError, WriterHandle},
    },
};
use anyhow::Context;
use interprocess::local_socket::tokio::prelude::*;
use process_path::get_executable_path;
use tokio::io::BufReader;
use tracing::{debug, trace};
use valuable::Valuable;

use crate::escalation::run_escalate;
use crate::escalation::Command;
use crate::run_mode::RunMode;
use crate::run_mode::RUN_MODE_ENV_NAME;
use crate::writer_process::ipc::{StatusMessage, WriterProcessConfig};

/// Handles the herding of all child processes. This includes lifecycle management
/// and communication.
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

        let args = serde_json::to_string(args)?;
        debug!(?args, "Converted WriterProcessConfig to JSON");

        let cmd = Command {
            proc: proc.to_string_lossy(),
            envs: vec![(RUN_MODE_ENV_NAME.into(), RunMode::Writer.as_str().into())],
            args: vec![
                args.into(),
                self.socket.socket_name().to_string_lossy().into(),
            ],
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
        let stream: LocalSocketStream = self.socket.accept().await?;
        let (rx, tx) = stream.split();
        let mut rx = Box::pin(BufReader::new(rx));
        let tx = Box::pin(tx);

        trace!("Reading results from child");
        let first_msg = read_msg_async::<StatusMessage>(&mut rx).await?;
        debug!(
            first_msg = first_msg.as_value(),
            "Read raw result from child"
        );

        let initial_info = match first_msg {
            StatusMessage::InitSuccess(i) => Ok(i),
            StatusMessage::Error(t) => Err(StartProcessError::Failed(Some(t))),
            other => Err(StartProcessError::UnexpectedFirstStatus(other)),
        }?;

        Ok(WriterHandle::new(Some(child), initial_info, rx, tx))
    }
}
