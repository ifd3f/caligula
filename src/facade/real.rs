use std::{path::PathBuf, time::Instant};

use futures::StreamExt;
use tokio::task::JoinHandle;

use crate::{
    facade::{
        DiskList, DiskWatcher, Escalator, FileAnalyzer, Orchestrator, WVState, WriteVerifyWorkflow,
        analyze_input::FileAnalysis,
        child::{ChildHerderClient, SpawnDaemonError},
        escalation::EscalationMethod,
        watch::Watch,
        workflow::hash::{self, HashWorkflow, HashingState},
    },
    herder_api::HerderService,
    util::stdiomux,
};

/// Actual CaligulaFacade implementation used by Caligula.
pub struct FacadeImpl {
    inner: tokio::sync::Mutex<Inner>,
}

struct Inner {
    log_path: String,

    child: SpawnedChild,
    escalated_child: Option<SpawnedChild>,
}

/// Bundle of the client and a [JoinHandle] to the driver future
struct SpawnedChild {
    client: ChildHerderClient,
    driver: JoinHandle<Result<(), stdiomux::client::ClientError>>,
}

impl Inner {
    fn pick_child_process(&self) -> &ChildHerderClient {
        if let Some(c) = self.escalated_child.as_ref() {
            &c.client
        } else {
            &self.child.client
        }
    }
}

impl FacadeImpl {
    pub async fn new(log_path: String) -> Result<Self, SpawnDaemonError> {
        let (client, driver) = super::child::spawn(log_path.clone(), false).await?;
        let driver = tokio::task::spawn_local(driver);

        Ok(Self {
            inner: Inner {
                log_path,
                child: SpawnedChild { client, driver },
                escalated_child: None,
            }
            .into(),
        })
    }
}

impl Orchestrator<WriteVerifyWorkflow> for FacadeImpl {
    #[tracing::instrument(skip_all)]
    async fn start_workflow(&self, params: WriteVerifyWorkflow) -> Watch<WVState> {
        tracing::info!("Requesting herder to start");

        let inner = self.inner.lock().await;

        let res = inner
            .pick_child_process()
            .start(params.make_child_config())
            .await;

        let res = match res {
            Ok(r) => r,
            Err(e) => {
                return Watch {
                    rx: tokio::sync::watch::channel(WVState::error(Instant::now(), e.into())).1,
                };
            }
        };

        tracing::info!(?res.start, "Got initial info from client");

        // create state reduction task
        let (tx_state, rx_state) = tokio::sync::watch::channel(WVState::initial(
            Instant::now(),
            !params.compression.is_identity(),
            res.start.input_file_bytes,
        ));

        let mut events = res.events;
        let _jh = tokio::task::spawn_local(async move {
            while !tx_state.borrow().is_finished() && !tx_state.is_closed() {
                let event = events.next().await;

                tx_state.send_modify(move |state| {
                    *state = std::mem::take(state).on_response(Instant::now(), event);
                });
            }
        });

        super::watch::Watch { rx: rx_state }
    }
}

impl Orchestrator<HashWorkflow> for FacadeImpl {
    async fn start_workflow(&self, workflow: HashWorkflow) -> Watch<HashingState> {
        let (w, _jh) = hash::run(workflow).await;
        w
    }
}

impl Escalator for FacadeImpl {
    async fn escalate(&self, _method: Option<EscalationMethod>) -> Result<(), SpawnDaemonError> {
        // TODO respect escalation method choice
        let mut inner = self.inner.lock().await;
        if inner.escalated_child.is_some() {
            return Ok(());
        }

        let (client, driver) = super::child::spawn(inner.log_path.clone(), true).await?;
        let driver = tokio::task::spawn_local(driver);
        inner.escalated_child = Some(SpawnedChild { client, driver });
        Ok(())
    }

    fn is_escalated(&self) -> bool {
        // TODO: this is badly implemented but it's good enough for writing new UIs
        // against. It will be improved when we get rid of herder facade.
        let Ok(lock) = self.inner.try_lock() else {
            return false;
        };
        lock.escalated_child.is_some()
    }
}

impl DiskWatcher for FacadeImpl {
    fn watch_disks(&self) -> Watch<DiskList> {
        unimplemented!(
            "Until this is implemented, for testing purposes, you may replace this with test \
             values."
        )
    }
}

impl FileAnalyzer for FacadeImpl {
    async fn analyze_file(&self, _input: PathBuf) -> std::io::Result<FileAnalysis> {
        unimplemented!(
            "Until this is implemented, for testing purposes, you may replace this with test \
             values."
        )
    }
}
