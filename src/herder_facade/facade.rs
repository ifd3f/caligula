use super::client::LazyHerderClient;
use super::{HerderFacade, StartWriterError, WriterHandle};
use crate::herder_daemon::ipc::{StatusMessage, WriterProcessConfig};
use crate::herder_facade::client::HerderClient as _;
use futures::StreamExt;
use std::collections::HashMap;
use std::hash::Hash;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_stream::wrappers::UnboundedReceiverStream;
use tracing::{debug, trace};

/// The actual [HerderFacade] used by Caligula.
pub struct HerderFacadeImpl {
    event_demux: Arc<std::sync::Mutex<EventDemuxMap<u64, StatusMessage>>>,
    next_writer_id: u64,

    standard_daemon: LazyHerderClient,
    escalated_daemon: LazyHerderClient,
}

impl HerderFacadeImpl {
    pub fn new(log_path: &str) -> Self {
        let event_demux = Arc::new(std::sync::Mutex::new(EventDemuxMap::new()));

        let cloned = event_demux.clone();
        let standard_daemon = LazyHerderClient::new(
            log_path.to_owned(),
            false,
            Box::new(move |e| {
                cloned.lock().unwrap().handle_event(e);
            }),
        );

        let cloned = event_demux.clone();
        let escalated_daemon = LazyHerderClient::new(
            log_path.to_owned(),
            true,
            Box::new(move |e| {
                cloned.lock().unwrap().handle_event(e);
            }),
        );

        Self {
            event_demux,
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
            let mut event_rx = UnboundedReceiverStream::new(
                self.event_demux
                    .lock()
                    .unwrap()
                    .take_receiver(id)
                    .expect("illegal state: receiver does not exist"),
            );

            let first_msg = event_rx.next().await;
            debug!(?first_msg, "Read raw result from child");

            let initial_info = match first_msg {
                Some(StatusMessage::InitSuccess(i)) => Ok(i),
                Some(StatusMessage::Error(t)) => Err(StartWriterError::Failed(Some(t))),
                Some(other) => Err(StartWriterError::UnexpectedFirstStatus(other)),
                None => Err(StartWriterError::UnexpectedDisconnect),
            }?;

            Ok(WriterHandle {
                events: Box::pin(event_rx),
                initial_info,
            })
        }
    }
}

#[derive(Debug)]
struct EventDemuxMap<K, T> {
    map: HashMap<K, (mpsc::UnboundedSender<T>, Option<mpsc::UnboundedReceiver<T>>)>,
}

impl<K: Hash + Eq, T> EventDemuxMap<K, T> {
    fn new() -> Self {
        Self {
            map: Default::default(),
        }
    }

    fn take_receiver(&mut self, id: K) -> Option<mpsc::UnboundedReceiver<T>> {
        self.map
            .entry(id)
            .or_insert_with(|| {
                let (tx, rx) = mpsc::unbounded_channel();
                (tx, Some(rx))
            })
            .1
            .take()
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
