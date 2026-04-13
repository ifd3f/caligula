use std::process::Stdio;

use super::evdist::EventDemux;
use crate::herder_daemon::ipc::StartHerd;
use crate::ipc_common::{read_msg_async, write_msg_async};
use crate::writer_process::ipc::ErrorType;
use futures::StreamExt;
use futures::stream::BoxStream;
use tokio::io::BufWriter;
use tokio::process::{Child, ChildStdin};
use tokio::sync::mpsc;
use tracing::{debug, trace};

use crate::escalation::run_escalate;
use crate::writer_process::ipc::{InitialInfo, StatusMessage, WriterProcessConfig};

type RawEventHandler = Box<dyn Fn((u64, StatusMessage)) + Send + 'static>;

/// Simple facade to an object that handles the herding of all child processes and subherds.
/// This includes lifecycle management and communication.
///
/// Why "Herder"? Caligula liked his horse, and horses are herded. I think. I'm not a farmer.
///
/// This is done so that we can easily test the UI as a separate component from the backend.
pub trait HerderFacade {
    fn start_writer(
        &mut self,
        args: &WriterProcessConfig,
        escalated: bool,
    ) -> impl Future<Output = Result<WriterHandle, StartWriterError>>;
}

/// The actual [HerderFacade] used by Caligula.
pub struct HerderFacadeImpl {
    event_demux: EventDemux<u64, StatusMessage>,
    next_writer_id: u64,

    standard_daemon: MaybeHerder,
    escalated_daemon: MaybeHerder,
}

impl HerderFacadeImpl {
    pub fn new(log_path: &str) -> Self {
        let (writer_tx, writer_rx) = mpsc::unbounded_channel();

        let cloned = writer_tx.clone();
        let standard_daemon = MaybeHerder::new(
            log_path.to_owned(),
            false,
            Box::new(move |e| {
                cloned.send(e).unwrap();
            }),
        );

        let escalated_daemon = MaybeHerder::new(
            log_path.to_owned(),
            true,
            Box::new(move |e| {
                writer_tx.send(e).unwrap();
            }),
        );

        Self {
            event_demux: EventDemux::new(writer_rx),
            next_writer_id: 0,
            standard_daemon,
            escalated_daemon,
        }
    }
}

impl HerderFacade for HerderFacadeImpl {
    fn start_writer(
        &mut self,
        args: &WriterProcessConfig,
        escalated: bool,
    ) -> impl Future<Output = Result<WriterHandle, StartWriterError>> {
        async move {
            let id = self.next_writer_id;
            self.next_writer_id += 1;

            let d = match escalated {
                true => &mut self.escalated_daemon,
                false => &mut self.standard_daemon,
            };

            d.start_writer(id, args).await?;

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
}

/// The actual [HerderFacade] used by Caligula.
struct MaybeHerder {
    log_path: String,
    escalated: bool,

    // very ugly but because of Polonius(tm) we have to implement this state machine as
    // taking eh and passing into daemon constructor
    eh: Option<RawEventHandler>,
    daemon: Option<HerderDaemonHandle>,
}

impl MaybeHerder {
    pub fn new(log_path: String, escalated: bool, eh: RawEventHandler) -> Self {
        Self {
            log_path,
            escalated,
            eh: Some(eh),
            daemon: None,
        }
    }

    #[tracing::instrument(skip_all)]
    async fn ensure_daemon(&mut self) -> Result<&mut HerderDaemonHandle, StartWriterError> {
        if let Some(eh) = self.eh.take() {
            self.daemon = Some(HerderDaemonHandle::new(&self.log_path, self.escalated, eh).await?);
        }
        Ok(self.daemon.as_mut().expect("This is an impossible state"))
    }

    fn start_writer(
        &mut self,
        id: u64,
        args: &WriterProcessConfig,
    ) -> impl Future<Output = Result<(), StartWriterError>> {
        async move {
            self.ensure_daemon()
                .await?
                .request_new_writer(id, args)
                .await?;
            Ok(())
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum StartWriterError {
    #[error("Unexpected first status: {0:?}")]
    UnexpectedFirstStatus(StatusMessage),
    #[error("Unexpectedly disconnected from writer")]
    UnexpectedDisconnect,
    #[error("Failed to spawn daemon (escalated={0:?}): {1:?}")]
    DaemonSpawnFailure(bool, anyhow::Error),
    #[error("Explicit failure signaled: {0:?}")]
    Failed(Option<ErrorType>),
    #[error("Error in transport: {0:?}")]
    TransportFailure(std::io::Error),
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
    ) -> Result<Self, StartWriterError> {
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
                .map_err(|e| StartWriterError::DaemonSpawnFailure(true, e.into()))?,
            false => {
                let mut c = tokio::process::Command::from(cmd);
                modify_cmd(&mut c);
                c.spawn()
                    .map_err(|e| StartWriterError::DaemonSpawnFailure(false, e.into()))?
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
    ) -> Result<(), StartWriterError> {
        write_msg_async(
            &mut self.tx,
            &StartHerd {
                id,
                action: args.clone(),
            },
        )
        .await
        .map_err(StartWriterError::TransportFailure)
    }
}

impl core::fmt::Debug for HerderDaemonHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("Handle").field(&self.child).finish()
    }
}
