use std::process::Stdio;
use std::sync::Arc;

use super::evdist::EventDemux;
use crate::herder_daemon::ipc::StartHerd;
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
pub struct HerderFacade {
    event_demux: EventDemux<u64, StatusMessage>,
    writer_tx: mpsc::UnboundedSender<(u64, StatusMessage)>,
    log_paths: Arc<LogPaths>,
    escalated_daemon: Option<HerderDaemonHandle>,
    next_writer_id: u64,
}

impl HerderFacade {
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
    async fn ensure_escalated_daemon(&mut self) -> anyhow::Result<&mut HerderDaemonHandle> {
        // Can't use if let here because of polonius! so we gotta do this ugly-ass workaround
        if self.escalated_daemon.is_none() {
            let tx = self.writer_tx.clone();
            self.escalated_daemon = Some(
                HerderDaemonHandle::new(self.log_paths.main(), true, move |e| {
                    tx.send(e).ok_or_log();
                })
                .await?,
            );
        }

        Ok(self.escalated_daemon.as_mut().unwrap())
    }

    #[tracing::instrument(skip_all, fields(escalate))]
    pub async fn start_writer(
        &mut self,
        args: &WriterProcessConfig,
        escalated: bool,
    ) -> anyhow::Result<WriterHandle> {
        let id = self.next_writer_id;
        self.next_writer_id += 1;

        if escalated {
            let daemon = self.ensure_escalated_daemon().await?;
            daemon
                .request_new_writer(id, args)
                .await
                .context("Failed to send while requesting new writer")?;
            None
        } else {
            let tx = self.writer_tx.clone();
            let cmd = spawn_writer(
                id,
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
            events: event_rx,
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

/// A wrapper around the events and information associated with a single writer
/// running inside a herder daemon.
pub struct WriterHandle {
    pub initial_info: InitialInfo,
    /// The stream of events from this daemon.
    pub events: BoxStream<'static, StatusMessage>,
}

/// A handle to a child process herder daemon.
///
/// If this is dropped, the child process inside is killed, if it manages one.
struct HerderDaemonHandle {
    /// We would like to kill the process on drop, if we are the direct parent of the
    /// process. So, we own a handle to it.
    pub(super) child: Option<Child>,
    pub(super) tx: BufWriter<ChildStdin>,
}

impl HerderDaemonHandle {
    async fn new(
        log_path: &str,
        escalated: bool,
        handle_event: impl Fn((u64, StatusMessage)) + Send + 'static,
    ) -> anyhow::Result<Self> {
        let proc = process_path::get_executable_path().unwrap();
        let cmd = crate::escalation::Command {
            proc: proc.to_str().unwrap().to_owned().into(),
            envs: vec![],
            args: vec!["_herder".into(), log_path.into()],
        };

        debug!("Starting child process with command: {:?}", cmd);
        fn modify_cmd(cmd: &mut tokio::process::Command) {
            cmd.kill_on_drop(true)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());
        }
        let mut child = match escalated {
            true => run_escalate(&cmd, modify_cmd)
                .await
                .context("Failed to spawn escalated daemon process")?,
            false => {
                let mut c = tokio::process::Command::from(cmd);
                modify_cmd(&mut c);
                c.spawn()
                    .context("Failed to spawn non-escalated daemon process")?
            }
        };

        // make the input pusher
        let child_rx = child.stdout.take().unwrap();
        tokio::spawn(async move {
            let mut child_rx = child_rx;
            loop {
                // TODO dont error here
                let msg = read_msg_async::<(u64, StatusMessage)>(&mut child_rx)
                    .await
                    .unwrap();
                handle_event(msg);
            }
        });

        debug!(?child, "Process spawned, waiting for pipe to be opened...");
        let child_tx = child.stdin.take().unwrap();

        Ok(Self {
            child: Some(child),
            tx: BufWriter::new(child_tx),
        })
    }

    async fn request_new_writer(
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

impl core::fmt::Debug for HerderDaemonHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("Handle").field(&self.child).finish()
    }
}
