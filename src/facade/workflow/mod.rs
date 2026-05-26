use std::error::Error;

use crate::facade::watch::Watch;

pub mod hash;
pub mod write_verify;

/// An abstract interface to the workflow spawning subsystem for a specific
/// workflow.
///
/// [`Workflow`]s represent a long-running, possibly multi-step series of tasks
/// that users would want to schedule, like write + verify, hash calculation,
/// and so on. This trait handles everything related to the workflow and exposes
/// it ti the UI:
///
/// - spawning child processes and escalating them
/// - orchestration of multiple steps
/// - reduction of event streams from the child processes into full states
///
/// Note that although some implementations spawn workers effectively
/// immediately, the actual interface is asynchronous. For synchronous UI
/// implementations, you should spawn a worker task as a shim between the
/// [`Orchestrator`] and your synchronous UI threads, probably using channels
/// and such to communicate.
pub trait Orchestrator<W: Workflow> {
    /// Start the workflow in the background.
    ///
    /// The workflow may have immediately returned an error. It is up to the
    /// caller to check and see if that is the case.
    async fn start_workflow(&self, workflow: W) -> Watch<W::State>;
}

/// Marker trait representing the parameters of a workflow that can be scheduled
/// to run in the background with an [`Orchestrator`].
pub trait Workflow {
    /// The state associated with this [`Workflow`].
    type State: WorkflowState;
}

/// A point-in-time representation of the state of an initialized [`Workflow`].
pub trait WorkflowState: Sync + 'static {
    /// Result of this workflow on success.
    type Success: Sync + 'static;

    /// Result of this workflow on failure.
    type Error: Sync + Error + 'static;

    /// Check this workflow for a result. If it returns [`None`], then this
    /// workflow is still running.
    fn result(&self) -> Option<&Result<Self::Success, Self::Error>>;
}

pub trait OrchestratorExt<W: Workflow>: Orchestrator<W> {
    /// Helper method for starting a workflow and checking if it immediately
    /// errored before continuing.
    async fn start_workflow_checked(
        &self,
        workflow: W,
    ) -> Result<Watch<W::State>, (Watch<W::State>, <W::State as WorkflowState>::Error)>
    where
        <W::State as WorkflowState>::Error: Clone,
    {
        let h = self.start_workflow(workflow).await;
        let err = match h.borrow().result() {
            Some(Err(e)) => Some(e.clone()),
            _ => None,
        };
        if let Some(err) = err {
            return Err((h, err));
        }
        Ok(h)
    }

    /// Like [`Self::start_workflow_checked()`] but it discards the [`Watch`] on
    /// error.
    async fn start_workflow_checked_discard(
        &self,
        workflow: W,
    ) -> Result<Watch<W::State>, <W::State as WorkflowState>::Error>
    where
        <W::State as WorkflowState>::Error: Clone,
    {
        self.start_workflow_checked(workflow)
            .await
            .map_err(|(_, e)| e)
    }
}

impl<O, W> OrchestratorExt<W> for O
where
    W: Workflow,
    O: Orchestrator<W>,
{
}
