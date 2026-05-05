use std::{error::Error, future, process::Stdio, sync::Arc};

use futures::{StreamExt, TryStreamExt, stream};
use stdiomux::{
    HandshakeError,
    mux::{BoxByteStream, basic::client::BasicMuxClientDriver},
};
use tokio::{
    io::{AsyncRead, AsyncWrite},
    process::Child,
};
use tower::{Service, ServiceExt};

use crate::{
    escalation::{EscalationError, run_escalate},
    herder_api::{HerdAction, HerdEvent, HerderService, Started, decode_response, serialize},
};

/// A low-level handle to a child process herder daemon.
///
/// If this is dropped, the child process inside is killed, if it manages one.
pub struct ChildHerderClient<S> {
    /// We would like to kill the process on drop, if we are the direct parent of the
    /// process. So, we own a handle to it.
    _child: Option<Child>,
    client: S,
}

impl<S> From<S> for ChildHerderClient<S>
where
    S: Service<BoxByteStream, Response = BoxByteStream>,
{
    fn from(client: S) -> Self {
        Self {
            _child: None,
            client,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum HerderClientError<H> {
    #[error("Transport error: {0}")]
    Transport(Arc<dyn Error>),
    #[error("Deserialization error: {0}")]
    Deserialization(#[from] postcard::Error),
    #[error("Herder error: {0}")]
    HerderError(H),
}

impl<A, S> HerderService<A> for ChildHerderClient<S>
where
    A: HerdAction,
    S: Service<BoxByteStream, Response = BoxByteStream>,
    S::Error: Error + 'static,
{
    type Error = HerderClientError<<A::Event as HerdEvent>::Failure>;

    async fn start(
        &mut self,
        action: A,
    ) -> Result<
        Started<<A::Event as HerdEvent>::StartInfo, Result<A::Event, Self::Error>>,
        Self::Error,
    > {
        let request = serialize(action).expect("unexpectedly failed to serialize action");
        let response = self
            .client
            .call(Box::pin(stream::once(future::ready(request))))
            .await
            .map_err(|e| HerderClientError::Transport(Arc::new(e)))?;

        let response = decode_response::<A>(response)
            .await?
            .map_err(HerderClientError::HerderError)?;

        Ok(Started {
            start: response.start,
            events: Box::pin(response.events.map_err(HerderClientError::Deserialization)),
        })
    }
}

/// Errors that occur while spawning the daemon.
#[derive(Debug, thiserror::Error)]
pub enum SpawnError {
    #[error("Failed to spawn child process: {0}")]
    Spawn(std::io::Error),
    #[error("Failed to escalate: {0}")]
    Escalation(#[from] EscalationError),
    #[error("Error during handshake: {0}")]
    Handshake(#[from] HandshakeError),
}

pub async fn spawn(
    log_path: String,
    escalated: bool,
) -> Result<
    (
        ChildHerderClient<impl Service<BoxByteStream, Response = BoxByteStream>>,
        BasicMuxClientDriver<impl AsyncRead + Unpin, impl AsyncWrite + Unpin>,
    ),
    SpawnError,
> {
    let proc = process_path::get_executable_path().unwrap();
    let cmd = crate::escalation::Command {
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
        true => run_escalate(&cmd, modify_cmd).await?,
        false => {
            let mut c = tokio::process::Command::from(cmd);
            modify_cmd(&mut c);
            c.spawn().map_err(SpawnError::Spawn)?
        }
    };

    let (c, f) = stdiomux::mux::basic::client::open(
        child.stdout.take().expect("should exist"),
        child.stdin.take().expect("should exist"),
    )
    .await?;

    let mut client = ChildHerderClient::from(c.map_response(|r| r.boxed()));
    client._child = Some(child);

    Ok((client, f))
}
