use crate::herder_daemon::ipc::{HerdAction, StartHerd};
use crate::herder_facade::{DaemonError, StartWriterError};
use crate::ipc_common::write_msg_async;
use tokio::io::AsyncWrite;
use tokio::process::Child;
use tracing::info;

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
pub(super) struct LazyHerderClient<F: HerderClientFactory> {
    // very ugly but because of Polonius(tm) we have to implement this state machine as
    // taking factory and passing into daemon constructor
    factory: Option<F>,
    daemon: Option<F::Output>,
}

/// For constructing [HerderClient]s.
///
/// Unfortunately I can't use an AsyncFnOnce raw because then I'll have so many ugly ugly ugly
/// explicit type holes and shit to patch in [LazyHerderClient] so this is the less bad option.
pub(super) trait HerderClientFactory {
    type Output: HerderClient;

    async fn make(self) -> Result<Self::Output, DaemonError>;
}

impl<H, F> HerderClientFactory for F
where
    H: HerderClient,
    F: AsyncFnOnce() -> Result<H, DaemonError>,
{
    type Output = H;

    async fn make(self) -> Result<Self::Output, DaemonError> {
        self().await
    }
}

impl<F: HerderClientFactory> LazyHerderClient<F> {
    pub fn new(factory: F) -> Self {
        Self {
            factory: Some(factory),
            daemon: None,
        }
    }

    #[tracing::instrument(skip_all)]
    async fn ensure_daemon(&mut self) -> Result<&mut F::Output, DaemonError> {
        if let Some(factory) = self.factory.take() {
            info!("spawning daemon from factory");
            let daemon = factory.make().await?;
            self.daemon = Some(daemon);
        }
        Ok(self.daemon.as_mut().expect("This is an impossible state"))
    }
}

impl<F: HerderClientFactory> HerderClient for LazyHerderClient<F> {
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
pub(super) struct RawHerderClient<W: AsyncWrite + Unpin> {
    /// We would like to kill the process on drop, if we are the direct parent of the
    /// process. So, we own a handle to it.
    pub(super) _child: Option<Child>,
    pub(super) tx: W,
}

impl<W: AsyncWrite + Unpin> From<W> for RawHerderClient<W> {
    fn from(tx: W) -> Self {
        Self { tx, _child: None }
    }
}

impl<W: AsyncWrite + Unpin> HerderClient for RawHerderClient<W> {
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

#[cfg(test)]
mod tests {
    // TODO
}
