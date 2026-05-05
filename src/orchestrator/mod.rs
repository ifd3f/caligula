//! Exposes the [`Orchestrator`], which is a facade that orchestrates all "high-level" work
//! and tracks the state of worker tasks.

pub use self::{
    herder_facade::{DaemonError, StartWriterError},
    write_verify::{WriteVerifyParams, WriteVerifyStarted, WriterState},
};
use crate::{escalation::EscalationMethod, herder_api::write_verify::*};

mod herder_facade;
mod real;
pub mod watch;
mod write_verify;

/// Main facade for UI implementations to interact with the rest of the program's logic.
///
/// This can be thought of as the glue between the UI and the backend, handling the following:
///
/// - spawning child processes and escalating them
/// - orchestrating multi-step workflows, such as write + verify
/// - reduction of event streams from the child processes into full states, along with [`Watch`]
///   handles for you to query state updates
///
/// Note that the interface is fully asynchronous. For synchronous UI implementations, you should
/// spawn a worker task as a shim between the [`Orchestrator`] and your synchronous UI threads,
/// probably using channels and such.
///
/// The API for this can be considered "mostly" stable. I'll be changing out the error types, but
/// in general, the overall shape of this API can be used for new UI developments.
pub trait Orchestrator {
    /// Start a write + verify workflow.
    ///
    /// Returns when we get an initial success message from the task group, or there was a failure.
    async fn start_write_verify(
        &self,
        begin_params: WriteVerifyParams,
    ) -> Result<WriteVerifyStarted, StartWriterError<WriteVerifyEvent>>;

    /// Attempt to spawn a child process as root using the provided escalation method (or [`None`] to
    /// automatically guess which one to use).
    ///
    /// Returns [`Ok`] if we successfully managed to escalate, or an error if we failed. If we were
    /// already escalated before this was called, returns [`Ok`].
    ///
    /// Once this is called, all future workflows will be routed through the escalated child process
    /// rather than executing at the parent's permission level!
    ///
    /// If your requested method involves the terminal, you should switch back to the non-alternate
    /// screen before calling this.
    async fn escalate(&mut self, method: Option<EscalationMethod>) -> Result<(), DaemonError>;

    /// Returns whether or not we have a child process running as root.
    #[expect(dead_code)]
    fn is_escalated(&self) -> bool;
}

/// Make the actual prod-used orchestrator implementation.
pub fn make_orchestrator_impl(log_path: &str) -> impl Orchestrator + Send + Sync + 'static {
    self::real::OrchestratorImpl::new(herder_facade::make_herder_facade_impl(log_path))
}
