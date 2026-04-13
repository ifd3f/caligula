//! Utilities for spawning and interacting with herder daemons.

mod client;
mod facade;

pub use facade::HerderFacadeImpl;
use futures::stream::BoxStream;

use crate::herder_daemon::ipc::{self, WriterProcessConfig};

/// Simple facade to an object that handles the herding of all child processes and subherds.
/// This includes lifecycle management and communication.
///
/// Why "Herder"? Caligula liked his horse, and horses are herded. I think. I'm not a farmer.
///
/// Making it a trait is so that we can easily test the UI as a separate component from the backend.
pub trait HerderFacade {
    fn start_writer(
        &mut self,
        args: &WriterProcessConfig,
        escalated: bool,
    ) -> impl Future<Output = Result<WriterHandle, StartWriterError>>;
}

/// A wrapper around the events and information associated with a single writer
/// running inside a herder daemon.
pub struct WriterHandle {
    pub initial_info: ipc::InitialInfo,
    /// The stream of events from this daemon.
    pub events: BoxStream<'static, ipc::StatusMessage>,
}

#[derive(Debug, thiserror::Error)]
pub enum StartWriterError {
    #[error("Unexpected first status: {0:?}")]
    UnexpectedFirstStatus(ipc::StatusMessage),
    #[error("Unexpectedly disconnected from writer")]
    UnexpectedDisconnect,
    #[error("Failed to spawn daemon (escalated={0:?}): {1:?}")]
    DaemonSpawnFailure(bool, anyhow::Error),
    #[error("Explicit failure signaled: {0:?}")]
    Failed(Option<ipc::ErrorType>),
    #[error("Error in transport: {0:?}")]
    TransportFailure(std::io::Error),
}
