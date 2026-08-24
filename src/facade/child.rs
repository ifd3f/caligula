use std::process::Stdio;

use bytes::Bytes;
use futures::{FutureExt as _, stream::BoxStream};
use tokio::{process::Child, try_join};

use super::escalation::{EscalationError, run_escalate};
use crate::{
    herder_api::{
        HerderAction, HerderResponse, HerderService, client::HerderClient, error::LayerError,
    },
};

type Client =
    HerderClient<RemoteThreadBytestreamClient<LocalBytestreamClient, BoxStream<'static, Bytes>>>;

#[derive(Debug, thiserror::Error)]
pub enum SpawnDaemonError {
    #[error("Failed to spawn escalated: {0}")]
    Escalation(#[from] EscalationError),
    #[error("Failed to spawn unescalated: {0}")]
    Standard(std::io::Error),
}

impl PartialEq for SpawnDaemonError {
    fn eq(&self, other: &Self) -> bool {
        matches!(
            (self, other),
            (Self::Escalation(_), Self::Escalation(_)) | (Self::Standard(_), Self::Standard(_))
        )
    }
}

/// Spawn a child process.
///
/// Returns the [`ChildHerderClient`], along with a driver future that must be
/// polled in the background in order for requests and responses to be handled.
#[tracing::instrument]
pub async fn spawn(
    log_path: String,
    escalated: bool,
) -> Result<
    (
        ChildHerderClient,
        impl Future<Output = Result<(), stdiomux::client::ClientError>>,
    ),
    SpawnDaemonError,
> {
    let proc = process_path::get_executable_path().unwrap();
    let cmd = super::escalation::Command {
        proc: proc.to_str().unwrap().to_owned().into(),
        envs: vec![],
        args: vec!["_herder".into(), log_path.into()],
    };

    tracing::debug!("Starting child process with command: {:?}", cmd);
    fn modify_cmd(cmd: &mut tokio::process::Command) {
        cmd.kill_on_drop(true)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
    }

    let mut child = match escalated {
        true => run_escalate(&cmd, modify_cmd)
            .await
            .map_err(SpawnDaemonError::Escalation)?,
        false => {
            let mut c = tokio::process::Command::from(cmd);
            modify_cmd(&mut c);
            c.spawn().map_err(SpawnDaemonError::Standard)?
        }
    };

    tracing::debug!(?child, "Process spawned");

    let child_rx = child.stdout.take().unwrap();
    let child_tx = child.stdin.take().unwrap();

    // open the client
    let (client, fut1) = stdiomux::client::open(child_rx, child_tx);

    // this client can only run on our current thread, create a remote handle for it
    let (client, fut2) = stdiomux::make_remote(client);

    // the driver future will drive both of these
    let fut = async move { try_join!(fut1, fut2.map(|_| Ok(()))).map(|_| ()) };

    Ok((
        ChildHerderClient {
            _child: child,
            client: HerderClient::new(client),
        },
        fut,
    ))
}

pub struct ChildHerderClient {
    _child: Child,
    client: Client,
}

impl<A: HerderAction> HerderService<A> for ChildHerderClient {
    type Error = <Client as HerderService<A>>::Error;

    async fn start(
        &self,
        action: A,
    ) -> Result<HerderResponse<A, Self::Error>, LayerError<A::Error, Self::Error>> {
        self.client.start(action).await
    }
}
