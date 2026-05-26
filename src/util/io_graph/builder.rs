use std::marker::PhantomData;

use tokio::sync::oneshot;

pub struct GraphContext<'a> {
    workers: Vec<WorkerEntry<'a>>,
}

pub struct WorkerInfo {
    pub name: String,
    pub kind: String,
}

struct WorkerEntry<'a> {
    info: WorkerInfo,
    ex: Box<dyn FnOnce() + 'a>,
}

impl<'a> GraphContext<'a> {
    pub fn add_worker<T, E>(
        &mut self,
        info: WorkerInfo,
        worker: impl FnOnce() -> Result<T, E> + 'a,
    ) -> impl Future<Output = Option<Result<T, E>>> + 'a
    where
        T: 'a,
        E: 'static,
    {
        let (tx, rx) = oneshot::channel();
        let entry = WorkerEntry {
            info,
            ex: Box::new(move || {
                let result = worker();
                tx.send(result).ok();
            }),
        };
        self.workers.push(entry);
        async move { rx.await.ok() }
    }
}
