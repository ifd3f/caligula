use super::client::MaybeHerder;
use super::{HerderFacade, StartWriterError, WriterHandle};
use crate::herder_daemon::ipc::{StatusMessage, WriterProcessConfig};
use futures::StreamExt;
use futures::stream::{self, BoxStream};
use std::collections::HashMap;
use std::hash::Hash;
use std::sync::Arc;
use tokio::sync::mpsc::{self, error::TryRecvError};
use tracing::{debug, trace};

/// The actual [HerderFacade] used by Caligula.
pub struct HerderFacadeImpl {
    event_demux: EventDemux<u64, StatusMessage>,
    next_writer_id: u64,

    standard_daemon: MaybeHerder,
    escalated_daemon: MaybeHerder,
}

impl HerderFacadeImpl {
    pub fn new(log_path: &str) -> Self {
        let (writer_tx, writer_rx) = mpsc::unbounded_channel();

        let cloned = writer_tx.clone();
        let standard_daemon = MaybeHerder::new(
            log_path.to_owned(),
            false,
            Box::new(move |e| {
                cloned.send(e).unwrap();
            }),
        );

        let escalated_daemon = MaybeHerder::new(
            log_path.to_owned(),
            true,
            Box::new(move |e| {
                writer_tx.send(e).unwrap();
            }),
        );

        Self {
            event_demux: EventDemux::new(writer_rx),
            next_writer_id: 0,
            standard_daemon,
            escalated_daemon,
        }
    }
}

impl HerderFacade for HerderFacadeImpl {
    fn start_writer(
        &mut self,
        args: &WriterProcessConfig,
        escalated: bool,
    ) -> impl Future<Output = Result<WriterHandle, StartWriterError>> {
        async move {
            let id = self.next_writer_id;
            self.next_writer_id += 1;

            let d = match escalated {
                true => &mut self.escalated_daemon,
                false => &mut self.standard_daemon,
            };

            d.start_writer(id, args).await?;

            trace!("Reading results from child");
            let mut event_rx = self.event_demux.select_key(id).unwrap();

            let first_msg = event_rx.next().await;
            debug!(?first_msg, "Read raw result from child");

            let initial_info = match first_msg {
                Some(StatusMessage::InitSuccess(i)) => Ok(i),
                Some(StatusMessage::Error(t)) => Err(StartWriterError::Failed(Some(t))),
                Some(other) => Err(StartWriterError::UnexpectedFirstStatus(other)),
                None => Err(StartWriterError::UnexpectedDisconnect),
            }?;

            Ok(WriterHandle {
                events: event_rx,
                initial_info,
            })
        }
    }
}

#[derive(Debug)]
pub struct EventDemux<K: Hash + Eq + Send, T: Send> {
    inner: Arc<std::sync::Mutex<EventDistributorInner<K, T>>>,
    rx: Arc<tokio::sync::Mutex<mpsc::UnboundedReceiver<(K, T)>>>,
}

impl<K: Hash + Eq + Send, T: Send> Clone for EventDemux<K, T> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            rx: self.rx.clone(),
        }
    }
}

impl<K: Hash + Eq + Send + 'static, T: Send + 'static> EventDemux<K, T> {
    pub fn new(rx: mpsc::UnboundedReceiver<(K, T)>) -> Self {
        Self {
            rx: Arc::new(tokio::sync::Mutex::new(rx)),
            inner: Arc::new(std::sync::Mutex::new(EventDistributorInner {
                map: HashMap::new(),
            })),
        }
    }

    pub fn select_key(&self, k: K) -> Option<BoxStream<'static, T>> {
        let Some(rx) = self.inner.lock().unwrap().get_receiver(k) else {
            return None;
        };

        let stream = stream::unfold((rx, self.clone()), move |(mut rx, this)| async move {
            loop {
                tokio::select! {
                    r = rx.recv() => {
                        let Some(m) = r else {
                            return None;
                        };
                        return Some((m, (rx, this)));
                    }
                    r = this.poll() => {
                        if !r {
                            return None;
                        }
                    }
                }
            }
        });

        Some(Box::pin(stream))
    }

    /// returns whether or not there are still events to listen to
    async fn poll(&self) -> bool {
        // wait until the next recv() occurs
        let mut rx = self.rx.lock().await;
        let Some(m) = rx.recv().await else {
            return false;
        };

        // fill the inner
        let mut inner = self.inner.lock().unwrap();
        inner.handle_event(m);
        inner.distribute_events(&mut rx);

        true
    }
}

#[derive(Debug)]
struct EventDistributorInner<K, T> {
    map: HashMap<K, (mpsc::UnboundedSender<T>, Option<mpsc::UnboundedReceiver<T>>)>,
}

impl<K: Hash + Eq, T> EventDistributorInner<K, T> {
    fn get_receiver(&mut self, id: K) -> Option<mpsc::UnboundedReceiver<T>> {
        self.map
            .entry(id)
            .or_insert_with(|| {
                let (tx, rx) = mpsc::unbounded_channel();
                (tx, Some(rx))
            })
            .1
            .take()
    }

    fn distribute_events(&mut self, rx: &mut mpsc::UnboundedReceiver<(K, T)>) -> bool {
        loop {
            match rx.try_recv() {
                Ok(m) => self.handle_event(m),
                Err(TryRecvError::Empty) => return true,
                Err(TryRecvError::Disconnected) => return false,
            };
        }
    }

    fn handle_event(&mut self, (k, t): (K, T)) {
        use std::collections::hash_map::Entry;
        match self.map.entry(k) {
            Entry::Occupied(e) => match e.get().0.send(t) {
                Ok(_) => (),
                Err(_) => {
                    e.remove();
                }
            },
            Entry::Vacant(e) => {
                let (tx, rx) = mpsc::unbounded_channel();
                match tx.send(t) {
                    Ok(_) => {
                        e.insert((tx, Some(rx)));
                    }
                    Err(_) => (),
                }
            }
        }
    }
}
