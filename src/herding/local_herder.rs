use std::{collections::BTreeMap, pin::Pin, sync::Arc, time::Duration};

use itertools::Itertools;
use tokio::{
    sync::{Mutex, mpsc},
    task::JoinHandle,
};
use tower::Service;
use uuid::Uuid;

use crate::herding::{Herder, WorkerUpdateBundle, worker_factory::WorkerFactory};

/// A [Herder] running locally, on this process.
pub struct LocalHerder<WF: WorkerFactory + 'static> {
    inner: Arc<HerderInner<WF>>,
    /// Handle to the task that updates and reports data
    updater: JoinHandle<()>,
}

struct HerderInner<WF: WorkerFactory + 'static> {
    wf: WF,
    children: Mutex<BTreeMap<Uuid, ChildWorker<WF::State>>>,
}

#[derive(Debug)]
struct ChildWorker<S> {
    handle: JoinHandle<()>,
    state: Arc<S>,
}

impl<WF: WorkerFactory + 'static> LocalHerder<WF> {
    /// Create a [LocalHerder] with the given [WorkerFactory], along with a period to poll
    /// workers for new updates.
    ///
    /// Returns the [LocalHerder] and a channel for receiving updates on.
    fn new(
        wf: WF,
        report_period: Duration,
    ) -> (
        Self,
        mpsc::UnboundedReceiver<WorkerUpdateBundle<WF::Report>>,
    ) {
        let inner = Arc::new(HerderInner {
            wf,
            children: Default::default(),
        });
        let (tx, rx) = mpsc::unbounded_channel();
        let updater = tokio::spawn(Self::run_updater(inner.clone(), tx, report_period));
        (LocalHerder { inner, updater }, rx)
    }

    async fn run_updater(
        inner: Arc<HerderInner<WF>>,
        tx: mpsc::UnboundedSender<WorkerUpdateBundle<WF::Report>>,
        report_period: Duration,
    ) {
        let mut interval = tokio::time::interval(report_period);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

        let mut prev_state: BTreeMap<Uuid, WF::Report> = BTreeMap::new();
        loop {
            let bundle = Self::run_reporter_once(&prev_state, inner.as_ref()).await;
            if let Err(_e) = tx.send(bundle.clone()) {
                break;
            }
            prev_state = bundle.updates;
            interval.tick().await;
        }
    }

    async fn run_reporter_once(
        prev_reports: &BTreeMap<Uuid, WF::Report>,
        inner: &HerderInner<WF>,
    ) -> WorkerUpdateBundle<WF::Report> {
        let current_state: Vec<(Uuid, WF::Report, bool)>;
        {
            let mut lock = inner.children.lock().await;

            // Summarize update info into tuples
            current_state = lock
                .iter()
                .map(|(uuid, worker)| {
                    (
                        *uuid,
                        inner.wf.report(worker.state.as_ref()),
                        worker.handle.is_finished(),
                    )
                })
                .collect_vec();

            // Reap finished workers
            lock.retain(|_, worker| !worker.handle.is_finished());
        };

        // Calculate and return the WorkerUpdateBundle

        let removals = current_state
            .iter()
            .filter(|(_, _, is_finished)| *is_finished)
            .map(|(uuid, _, _)| *uuid)
            .collect();

        let updates = current_state
            .into_iter()
            .filter_map(|(uuid, new, _)| match prev_reports.get(&uuid) {
                Some(prev) if prev == &new => None,
                _ => Some((uuid, new)),
            })
            .collect();

        WorkerUpdateBundle { updates, removals }
    }
}

impl<WF: WorkerFactory + 'static> HerderInner<WF> {
    async fn handle(&self, params: WF::Params) -> Result<(Uuid, WF::Response), WF::Error> {
        let worker_spawned = self.wf.spawn(params).await?;
        let uuid = Uuid::new_v4();

        self.children.lock().await.insert(
            uuid,
            ChildWorker {
                handle: tokio::spawn(worker_spawned.future),
                state: worker_spawned.state,
            },
        );

        Ok((uuid, worker_spawned.response))
    }
}

impl<WF> Service<WF::Params> for LocalHerder<WF>
where
    WF: WorkerFactory + 'static,
{
    type Response = (Uuid, WF::Response);

    type Error = WF::Error;

    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>>>>;

    fn poll_ready(
        &mut self,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        std::task::Poll::Ready(Ok(()))
    }

    fn call(&mut self, params: WF::Params) -> Self::Future {
        let inner = self.inner.clone();
        Box::pin(async move { inner.handle(params).await })
    }
}

impl<WF> Herder<WF> for LocalHerder<WF> where WF: WorkerFactory + 'static {}
