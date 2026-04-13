use crate::escalation::run_escalate;
use crate::herder_daemon::ipc::StartHerd;
use crate::herder_daemon::ipc::{StatusMessage, WriterProcessConfig};
use crate::herder_facade::StartWriterError;
use crate::ipc_common::{read_msg_async, write_msg_async};
use std::process::Stdio;
use tokio::io::BufWriter;
use tokio::process::{Child, ChildStdin};
use tracing::debug;

type RawEventHandler = Box<dyn Fn((u64, StatusMessage)) + Send + 'static>;

/// The actual [HerderFacade] used by Caligula.
pub(super) struct MaybeHerder {
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

    pub fn start_writer(
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

/// A handle to a child process herder daemon.
///
/// If this is dropped, the child process inside is killed, if it manages one.
pub(super) struct HerderDaemonHandle {
    /// We would like to kill the process on drop, if we are the direct parent of the
    /// process. So, we own a handle to it.
    pub(super) _child: Option<Child>,
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
            _child: Some(child),
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
