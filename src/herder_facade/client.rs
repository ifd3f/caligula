use crate::escalation::run_escalate;
use crate::herder_daemon::ipc::{HerdAction, StartHerd, TopLevelHerdEvent};
use crate::herder_facade::{DaemonError, StartWriterError};
use crate::ipc_common::{read_msg_async, write_msg_async};
use std::process::Stdio;
use tokio::io::BufWriter;
use tokio::process::{Child, ChildStdin};
use tracing::debug;

/// A very raw, low-level, write-only interface to the herder daemon.
/// Literally doesn't even implement responses.
pub(super) trait HerderClient {
    async fn start_writer<A: HerdAction>(
        &mut self,
        id: u64,
        action: A,
    ) -> Result<(), StartWriterError<A::Event>>;
}

/// A [HerderClient] that doesn't actually spawn the real [HerderClient] until it
/// gets the first request.
pub(super) struct LazyHerderClient<H, F>
where
    H: HerderClient,
    F: HerderClientFactory<Output = H>,
{
    // very ugly but because of Polonius(tm) we have to implement this state machine as
    // taking factory and passing into daemon constructor
    factory: Option<F>,
    daemon: Option<H>,
}

/// For constructing [HerderClient]s.
///
/// Unfortunately I can't use an AsyncFnOnce because then I'll have so many ugly ugly ugly
/// explicit type holes and shit to patch in [LazyHerderClient] so this is the less bad option.
pub(super) trait HerderClientFactory {
    type Output: HerderClient;
    async fn make(self) -> Result<Self::Output, DaemonError>;
}

impl<H, F> LazyHerderClient<H, F>
where
    H: HerderClient,
    F: HerderClientFactory<Output = H>,
{
    pub fn new(factory: F) -> Self {
        Self {
            factory: Some(factory),
            daemon: None,
        }
    }

    #[tracing::instrument(skip_all)]
    async fn ensure_daemon(&mut self) -> Result<&mut H, DaemonError> {
        if let Some(factory) = self.factory.take() {
            let daemon = factory.make().await?;
            self.daemon = Some(daemon);
        }
        Ok(self.daemon.as_mut().expect("This is an impossible state"))
    }
}

impl<H, F> HerderClient for LazyHerderClient<H, F>
where
    H: HerderClient,
    F: HerderClientFactory<Output = H>,
{
    async fn start_writer<A: HerdAction>(
        &mut self,
        id: u64,
        action: A,
    ) -> Result<(), StartWriterError<A::Event>> {
        self.ensure_daemon().await?.start_writer(id, action).await?;
        Ok(())
    }
}

/// A low-level handle to a child process herder daemon.
///
/// If this is dropped, the child process inside is killed, if it manages one.
pub(super) struct RawHerderClient {
    /// We would like to kill the process on drop, if we are the direct parent of the
    /// process. So, we own a handle to it.
    pub(super) _child: Option<Child>,
    pub(super) tx: BufWriter<ChildStdin>,
}

impl RawHerderClient {
    pub(super) async fn spawn_herder(
        log_path: &str,
        escalated: bool,
        handle_event: impl Fn((u64, TopLevelHerdEvent)) + Send + 'static,
    ) -> Result<Self, DaemonError> {
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
                .map_err(|e| DaemonError::DaemonSpawnFailure(true, e.into()))?,
            false => {
                let mut c = tokio::process::Command::from(cmd);
                modify_cmd(&mut c);
                c.spawn()
                    .map_err(|e| DaemonError::DaemonSpawnFailure(false, e.into()))?
            }
        };

        // make the input pusher
        let child_rx = child.stdout.take().unwrap();
        tokio::spawn(async move {
            let mut child_rx = child_rx;
            loop {
                // TODO dont error here
                let msg = read_msg_async::<(u64, TopLevelHerdEvent)>(&mut child_rx)
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
}

impl HerderClient for RawHerderClient {
    async fn start_writer<A: HerdAction>(
        &mut self,
        id: u64,
        action: A,
    ) -> Result<(), StartWriterError<A::Event>> {
        write_msg_async(&mut self.tx, &StartHerd { id, action })
            .await
            .map_err(DaemonError::TransportFailure)?;
        Ok(())
    }
}
