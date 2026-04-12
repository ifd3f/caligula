use std::borrow::Cow;
use std::process::Stdio;
use std::sync::Arc;

use crate::herder_daemon::ipc::StartHerd;
use crate::evdist::EventDemux;
use crate::ipc_common::{read_msg_async, write_msg_async};
use crate::logging::LogPaths;
use crate::writer_process::ipc::ErrorType;
use crate::writer_process::spawn_writer;
use anyhow::Context;
use futures::StreamExt;
use futures::stream::BoxStream;
use tokio::io::BufWriter;
use tokio::process::{Child, ChildStdin};
use tokio::sync::mpsc;
use tracing::{debug, trace};
use tracing_unwrap::ResultExt;

use crate::escalation::run_escalate;
use crate::writer_process::ipc::{InitialInfo, StatusMessage, WriterProcessConfig};

/// Handles the herding of all child processes. This includes lifecycle management
/// and communication.
///
/// Why "Herder"? Caligula liked his horse, and horses are herded. I think. I'm not
/// a farmer.
pub struct Herder {
    event_demux: EventDemux<u64, StatusMessage>,
    writer_tx: mpsc::UnboundedSender<(u64, StatusMessage)>,
    log_paths: Arc<LogPaths>,
    escalated_daemon: Option<EscDaemonHandle>,
    next_writer_id: u64,
}

impl Herder {
    pub fn new(log_paths: Arc<LogPaths>) -> Self {
        let (writer_tx, writer_rx) = mpsc::unbounded_channel();
        Self {
            escalated_daemon: None,
            log_paths,
            next_writer_id: 0,
            event_demux: EventDemux::new(writer_rx),
            writer_tx,
        }
    }

    #[tracing::instrument(skip_all)]
    async fn ensure_escalated_daemon(&mut self) -> anyhow::Result<&mut EscDaemonHandle> {
        // Can't use if let here because of polonius! so we gotta do this ugly-ass workaround
        if self.escalated_daemon.is_none() {
            let log_path = self.log_paths.escalated_daemon();
            let cmd = make_escalated_daemon_spawn_command(log_path.to_string_lossy());

            debug!("Starting child process with command: {:?}", cmd);
            fn modify_cmd(cmd: &mut tokio::process::Command) {
                cmd.kill_on_drop(true)
                    .stdin(Stdio::piped())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped());
            }
            let mut child = run_escalate(&cmd, modify_cmd)
                .await
                .context("Failed to spawn escalated daemon process")?;

            // make the input pusher
            let child_rx = child.stdout.take().unwrap();
            let event_tx = self.writer_tx.clone();
            tokio::spawn(async move {
                let mut child_rx = child_rx;
                loop {
                    // TODO dont error here
                    let msg = read_msg_async::<(u64, StatusMessage)>(&mut child_rx)
                        .await
                        .unwrap();
                    event_tx.send(msg).unwrap();
                }
            });

            debug!(?child, "Process spawned, waiting for pipe to be opened...");
            let child_tx = child.stdin.take().unwrap();
            let handle = EscDaemonHandle::new(Some(child), child_tx);

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
        let id = self.next_writer_id;
        self.next_writer_id += 1;

        if escalate {
            let daemon = self.ensure_escalated_daemon().await?;
            daemon
                .request_new_writer(id, args)
                .await
                .context("Failed to send while requesting new writer")?;
            None
        } else {
            let tx = self.writer_tx.clone();
            let cmd = spawn_writer(
                move |m| {
                    tx.send((id, m)).ok_or_log();
                },
                args.clone(),
            );

            Some(cmd)
        };

        trace!("Reading results from child");
        let mut event_rx = self.event_demux.select_key(id).unwrap();

        let first_msg = event_rx.next().await;
        debug!(?first_msg, "Read raw result from child");

        let initial_info = match first_msg {
            Some(StatusMessage::InitSuccess(i)) => Ok(i),
            Some(StatusMessage::Error(t)) => Err(StartWriterError::Failed(Some(t))),
            Some(other) => Err(StartWriterError::UnexpectedFirstStatus(other)),
            None => Err(StartWriterError::UnexpectedDisconnect),
        }?;

        Ok(WriterHandle {
            event_rx,
            initial_info,
        })
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum StartWriterError {
    #[error("Unexpected first status: {0:?}")]
    UnexpectedFirstStatus(StatusMessage),
    #[error("Unexpectedly disconnected from writer")]
    UnexpectedDisconnect,
    #[error("Explicit failure signaled: {0:?}")]
    Failed(Option<ErrorType>),
}

/// A wrapper around a [ChildHandle].
pub struct WriterHandle {
    pub(super) event_rx: BoxStream<'static, StatusMessage>,
    pub(super) initial_info: InitialInfo,
}

impl WriterHandle {
    pub async fn next_message(&mut self) -> anyhow::Result<Option<StatusMessage>> {
        // TODO: is this Result even necessary????
        Ok(self.event_rx.next().await)
    }

    pub fn initial_info(&self) -> &InitialInfo {
        &self.initial_info
    }
}

/// A very low-level handle for interacting with a child process connected to our socket.
///
/// If this is dropped, the child process inside is killed, if it manages one.
struct EscDaemonHandle {
    /// We would like to kill the process on drop, if we are the direct parent of the
    /// process. So, we own a handle to it.
    pub(super) child: Option<Child>,
    pub(super) tx: BufWriter<ChildStdin>,
}

impl EscDaemonHandle {
    pub fn new(child: Option<Child>, tx: ChildStdin) -> EscDaemonHandle {
        Self {
            child,
            tx: BufWriter::new(tx),
        }
    }

    pub async fn request_new_writer(
        &mut self,
        id: u64,
        args: &WriterProcessConfig,
    ) -> Result<(), std::io::Error> {
        write_msg_async(
            &mut self.tx,
            &StartHerd {
                id,
                action: args.clone(),
            },
        )
        .await
    }
}

impl core::fmt::Debug for EscDaemonHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("Handle").field(&self.child).finish()
    }
}

/// Build a [Command] that, when run, spawns a process with a specific configuration.
pub fn make_escalated_daemon_spawn_command<'a>(
    log_path: Cow<'a, str>,
) -> crate::escalation::Command<'a> {
    let proc = process_path::get_executable_path().unwrap();

    crate::escalation::Command {
        proc: proc.to_str().unwrap().to_owned().into(),
        envs: vec![],
        args: vec!["_herder".into(), log_path],
    }
}
