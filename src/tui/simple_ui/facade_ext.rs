use std::sync::Arc;

use crate::{
    facade::{
        CaligulaFacade, EscalationMethod, OrchestratorExt, SpawnDaemonError, WVState,
        WriteVerifyWorkflow, WriteVerifyWorkflowError, watch::Watch,
    },
    util::runtime::RemoteSpawn,
};

pub trait FacadeExt: CaligulaFacade + OrchestratorExt<WriteVerifyWorkflow> {
    /// Like [`CaligulaFacade::start_write_verify()`], but it blocks your thread
    /// while waiting for it to start.
    ///
    /// THIS SHOULD ABSOLUTELY NOT UNDER ANY CIRCUMSTANCES be used in an async
    /// context! Just use the non-blocking version of the trait! It's mostly
    /// only useful for the simple UI wizard, which is inherently blocky.
    fn start_write_verify_blocking(
        self: Arc<Self>,
        spawn: impl RemoteSpawn,
        params: WriteVerifyWorkflow,
    ) -> Result<Watch<WVState>, Arc<WriteVerifyWorkflowError>> {
        spawn
            .spawn(move || async move {
                self.as_ref()
                    .start_workflow_checked(params)
                    .await
                    .map_err(|(_, e)| e)
            })
            .blocking_recv()
            .expect("remote task dropped!")
    }

    /// Like [`CaligulaFacade::escalate()`], but it blocks your thread while
    /// waiting for it to start.
    ///
    /// THIS SHOULD ABSOLUTELY NOT UNDER ANY CIRCUMSTANCES be used in an async
    /// context! Just use the non-blocking version of the trait! It's mostly
    /// only useful for the simple UI wizard, which is inherently blocky.
    fn escalate_blocking(
        self: Arc<Self>,
        spawn: impl RemoteSpawn,
        method: Option<EscalationMethod>,
    ) -> Result<(), SpawnDaemonError> {
        spawn
            .spawn(move || async move { self.escalate(method).await })
            .blocking_recv()
            .expect("remote task dropped!")
    }
}

impl<O: CaligulaFacade + OrchestratorExt<WriteVerifyWorkflow>> FacadeExt for O {}
