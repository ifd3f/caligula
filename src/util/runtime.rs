use futures::future::LocalBoxFuture;
use tokio::{
    sync::{mpsc, oneshot},
    task::LocalSet,
};

pub type FutureFactory = Box<dyn FnOnce() -> LocalBoxFuture<'static, ()> + Send>;

/// Abstract interface for a single-threaded async runtime running on a
/// different thread.
pub trait RemoteSpawn {
    /// Schedule a future to be spawned on a remote async thread, and return a
    /// handle for awaiting its completion.
    ///
    /// Notably, although the function that creates the future needs to be
    /// [`Send`], the future itself does not need to be [`Send`].
    fn spawn<Fut, T>(&self, f: impl FnOnce() -> Fut + Send + 'static) -> oneshot::Receiver<T>
    where
        T: Send + 'static,
        Fut: Future<Output = T> + 'static;
}

impl<Sp: RemoteSpawn> RemoteSpawn for &Sp {
    fn spawn<Fut, T>(&self, f: impl FnOnce() -> Fut + Send + 'static) -> oneshot::Receiver<T>
    where
        T: Send + 'static,
        Fut: Future<Output = T> + 'static,
    {
        (*self).spawn(f)
    }
}

/// Object that manages the async runtime thread in the background.
///
/// Attempts to terminate the runtime on drop.
pub struct AsyncRuntime {
    mailbox: mpsc::Sender<FutureFactory>,
    _handle: tokio::runtime::Handle,
    _thread: Option<std::thread::JoinHandle<()>>,
}

impl AsyncRuntime {
    /// Start an async runtime in the background.
    pub fn start() -> Self {
        // build runtime and get handle
        let (handle_tx, handle_rx) = oneshot::channel();
        let (tx, mut rx) = mpsc::channel::<FutureFactory>(10);

        // actually spawn the thread
        let thread = std::thread::Builder::new()
            .name("tokio".into())
            .spawn(move || {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("Failed to create tokio runtime!");
                handle_tx
                    .send(runtime.handle().clone())
                    .expect("handle was not sent");

                runtime.block_on(LocalSet::new().run_until(async move {
                    while let Some(task) = rx.recv().await {
                        tracing::debug!("Got async task in mailbox, spawning it");
                        let fut = task();
                        tokio::task::spawn_local(fut);
                    }
                }))
            })
            .expect("Failed to spawn async runtime thread!");

        Self {
            mailbox: tx,
            _handle: handle_rx.blocking_recv().expect("handle was not sent"),
            _thread: Some(thread),
        }
    }
}

impl RemoteSpawn for AsyncRuntime {
    fn spawn<Fut, T>(&self, f: impl FnOnce() -> Fut + Send + 'static) -> oneshot::Receiver<T>
    where
        T: Send + 'static,
        Fut: Future<Output = T> + 'static,
    {
        let (tx, rx) = oneshot::channel();
        self.mailbox
            .blocking_send(Box::new(move || {
                let fut = f();
                let wrapped = async move {
                    tx.send(fut.await).ok();
                };
                Box::pin(wrapped)
            }))
            .expect("spawner dropped!");
        rx
    }
}
