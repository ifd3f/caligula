use std::{
    path::PathBuf,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Instant,
};

use futures::StreamExt;
use tokio::{
    sync::{mpsc, oneshot},
    task::spawn_local,
};

use crate::{
    facade::{
        CaligulaFacade, DiskList, DiskWatcher, Escalator, FileAnalyzer, Orchestrator, WVState,
        WriteVerifyWorkflow,
        analyze_input::FileAnalysis,
        child::{ChildHerderClient, SpawnDaemonError},
        escalation::EscalationMethod,
        watch::Watch,
        workflow::hash::{self, HashWorkflow, HashingState},
    },
    herder_api::HerderService,
    util::runtime::RemoteSpawn,
};

/// Make the actual prod-used CaligulaFacade implementation.
pub fn make_real_facade(
    log_path: String,
    runtime: impl RemoteSpawn,
) -> Result<impl CaligulaFacade, SpawnDaemonError> {
    let (mailbox_tx, mailbox_rx) = mpsc::unbounded_channel::<AsyncContextTask>();
    let shared = Arc::new(Shared {
        log_path,
        escalated: false.into(),
    });

    runtime.spawn({
        let shared = shared.clone();
        move || facade_actor(shared, mailbox_rx)
    });

    let facade = FacadeImpl {
        handle: AsyncContextHandle {
            mailbox: mailbox_tx,
        },
        shared,
    };

    Ok(facade)
}

/// Actual CaligulaFacade implementation used by Caligula.
struct FacadeImpl {
    handle: AsyncContextHandle,
    shared: Arc<Shared>,
}

/// Type alias for a closure that can be sent to the facade helper actor.
type AsyncContextTask = Box<dyn FnOnce(&Rc<AsyncContext>) + Send + 'static>;

/// Simple wrapper for scheduling work on the async runtime.
struct AsyncContextHandle {
    mailbox: mpsc::UnboundedSender<AsyncContextTask>,
}

impl AsyncContextHandle {
    /// Schedule a task to be run on the async thread in the background. This
    /// lets you access the [`AsyncContext`] and spawn_local.
    fn schedule<T: Send + 'static>(
        &self,
        f: impl AsyncFnOnce(Rc<AsyncContext>) -> T + Send + 'static,
    ) -> oneshot::Receiver<T> {
        let (handle_tx, handle_rx) = oneshot::channel();

        self.mailbox
            .send(Box::new(move |ctx| {
                let ctx = ctx.clone();
                spawn_local(async move {
                    let res = f(ctx).await;
                    handle_tx.send(res).ok();
                });
            }))
            .expect("async runtime crashed");

        handle_rx
    }
}

/// Plain data shared between the async thread and the facade.
struct Shared {
    log_path: String,

    /// Whether or not we have successfully escalated.
    escalated: AtomicBool,
}

/// Context object that lives inside the async runtime's thread.
struct AsyncContext {
    child: ChildHerderClient,
    escalated_child: tokio::sync::OnceCell<ChildHerderClient>,
}

/// Actor task that the facade talks to for doing things on the async runtime
/// thread.
#[tracing::instrument(skip_all)]
async fn facade_actor(
    shared: Arc<Shared>,
    mut mailbox: mpsc::UnboundedReceiver<AsyncContextTask>,
) -> Result<(), SpawnDaemonError> {
    let (child, fut) = super::child::spawn(shared.log_path.clone(), false).await?;
    tokio::task::spawn_local(fut);

    let ctx = Rc::new(AsyncContext {
        child,
        escalated_child: tokio::sync::OnceCell::new(),
    });

    while let Some(f) = mailbox.recv().await {
        f(&ctx);
    }

    Ok(())
}

impl AsyncContext {
    fn pick_child_process(&self) -> &ChildHerderClient {
        if let Some(c) = self.escalated_child.get() {
            c
        } else {
            &self.child
        }
    }
}

impl Orchestrator<WriteVerifyWorkflow> for FacadeImpl {
    #[tracing::instrument(skip_all)]
    async fn start_workflow(&self, params: WriteVerifyWorkflow) -> Watch<WVState> {
        tracing::info!("Requesting herder to start");

        let action = move |ctx: Rc<AsyncContext>| async move {
            let res = ctx
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
        };

        self.handle
            .schedule(action)
            .await
            .expect("async runtime crashed")
    }
}

impl Orchestrator<HashWorkflow> for FacadeImpl {
    async fn start_workflow(&self, workflow: HashWorkflow) -> Watch<HashingState> {
        self.handle
            .schedule(move |_| async move {
                let (w, _jh) = hash::run(workflow).await;
                w
            })
            .await
            .expect("async runtime crashed")
    }
}

impl Escalator for FacadeImpl {
    async fn escalate(&self, _method: Option<EscalationMethod>) -> Result<(), SpawnDaemonError> {
        // TODO respect escalation method choice

        let shared = self.shared.clone();

        let action = move |ctx: Rc<AsyncContext>| async move {
            let res = ctx
                .escalated_child
                .get_or_try_init(move || async move {
                    let (child, fut) = super::child::spawn(shared.log_path.clone(), true).await?;
                    tokio::task::spawn_local(fut);
                    shared.escalated.store(true, Ordering::Relaxed);
                    Ok(child)
                })
                .await;

            res.map(|_| ())
        };

        self.handle
            .schedule(action)
            .await
            .expect("async thread dropped")
    }

    fn is_escalated(&self) -> bool {
        self.shared.escalated.load(Ordering::Relaxed)
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
