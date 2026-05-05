//! WARNING: HERE THERE BE DRAGONS
//!
//! The good parts of this submodule will get assimilated into orchestrator once I get back to working
//! on the stdiomux branch. Don't rely on this module whatsoever! Orchestrator is mildly stable though.

mod client;
mod facade;

use futures::stream::BoxStream;

use crate::herder_api::{HerdAction, HerdEvent, TopLevelHerdEvent};

pub use facade::make_herder_facade_impl;

/// Simple facade to an object that handles the herding of all child processes and subherds.
/// This includes lifecycle management and communication.
///
/// Why "Herder"? Caligula liked his horse, and horses are herded. I think. I'm not a farmer.
///
/// Making it a trait is so that we can easily test the UI as a separate component from the backend.
pub trait HerderFacade {
    async fn start_herd<A: HerdAction>(
        &mut self,
        action: A,
        escalated: bool,
    ) -> Result<HerdHandle<A::Event>, StartWriterError<A::Event>>;

    async fn ensure_escalated_daemon(&mut self) -> Result<(), DaemonError>;
}

/// A wrapper around the events and information associated with a single herd
/// running inside a herder daemon.
pub struct HerdHandle<E: HerdEvent> {
    pub initial_info: E::StartInfo,
    /// The stream of events from this daemon.
    pub events: BoxStream<'static, E>,
}

#[derive(Debug, thiserror::Error)]
pub enum StartWriterError<E: HerdEvent> {
    #[error("Unexpected first status: {0:?}")]
    UnexpectedFirstStatus(E),
    #[error("Explicit error signaled: {0}")]
    Failed(E::Failure),
    #[error("Daemon management error: {0}")]
    DaemonError(#[from] DaemonError),
}

#[derive(Debug, thiserror::Error)]
pub enum DaemonError {
    #[error("Unexpectedly disconnected from writer")]
    UnexpectedDisconnect,
    #[error("Failed to spawn daemon (escalated={0:?}): {1}")]
    DaemonSpawnFailure(bool, anyhow::Error),
    #[error("Error in transport: {0:?}")]
    TransportFailure(std::io::Error),
    #[error("Unexpected event type: {0:?}")]
    UnexpectedEventType(TopLevelHerdEvent),
}
