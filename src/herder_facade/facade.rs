use super::client::LazyHerderClient;
use super::{HerdHandle, HerderFacade, StartWriterError};
use crate::herder_daemon::ipc::{HerdAction, HerdEvent, TopLevelHerdEvent};
use crate::herder_facade::DaemonError;
use crate::herder_facade::client::{HerderClient, HerderClientFactory, RawHerderClient};
use futures::StreamExt;
use std::collections::HashMap;
use std::hash::Hash;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_stream::wrappers::UnboundedReceiverStream;
use tracing::{debug, trace};
use tracing_unwrap::ResultExt;

/// Make the actual prod-used [HerderFacade].
///
/// Doing it this way with a function is so that we can hide all of those ugly ugly ugly
/// type signatures under a nice `impl HerderFacade + 'static`!
pub fn make_herder_facade_impl(log_path: &str) -> impl HerderFacade + 'static {
    let event_demux = Arc::new(std::sync::Mutex::new(EventDemuxMap::new()));

    /// Simple implementor of [HerderClientFactory].
    struct ImplFactory {
        log_path: String,
        event_demux: Arc<std::sync::Mutex<EventDemuxMap<u64, TopLevelHerdEvent>>>,
        escalated: bool,
    }

    impl HerderClientFactory for ImplFactory {
        type Output = RawHerderClient;

        async fn make(self) -> Result<Self::Output, DaemonError> {
            Ok(RawHerderClient::spawn_herder(
                &self.log_path,
                self.escalated,
                Box::new(move |e| {
                    self.event_demux.lock().unwrap().handle_event(e);
                }),
            )
            .await?)
        }
    }
    let standard_daemon = LazyHerderClient::new(ImplFactory {
        log_path: log_path.to_owned(),
        event_demux: event_demux.clone(),
        escalated: false,
    });
    let escalated_daemon = LazyHerderClient::new(ImplFactory {
        log_path: log_path.to_owned(),
        event_demux: event_demux.clone(),
        escalated: true,
    });

    HerderFacadeImpl {
        event_demux,
        next_writer_id: 0,
        standard_daemon,
        escalated_daemon,
    }
}

/// Implementation of the actual [HerderFacade] used by Caligula.
struct HerderFacadeImpl<Std, Esc> {
    event_demux: Arc<std::sync::Mutex<EventDemuxMap<u64, TopLevelHerdEvent>>>,
    next_writer_id: u64,

    standard_daemon: Std,
    escalated_daemon: Esc,
}

impl<Std, Esc> HerderFacade for HerderFacadeImpl<Std, Esc>
where
    Std: HerderClient,
    Esc: HerderClient,
{
    async fn start_herd<A: HerdAction>(
        &mut self,
        args: A,
        escalated: bool,
    ) -> Result<HerdHandle<A::Event>, StartWriterError<A::Event>> {
        let id = self.next_writer_id;
        self.next_writer_id += 1;

        match escalated {
            true => self.escalated_daemon.start_writer(id, args).await?,
            false => self.standard_daemon.start_writer(id, args).await?,
        }

        trace!("Reading results from child");
        let mut event_rx = UnboundedReceiverStream::new(
            self.event_demux
                .lock()
                .unwrap()
                .take_receiver(id)
                .expect("illegal state: receiver does not exist"),
        )
        .filter_map(|event| {
            std::future::ready(
                A::Event::try_from(event)
                    .map_err(DaemonError::UnexpectedEventType)
                    .ok_or_log(),
            )
        });

        let first_msg = event_rx
            .next()
            .await
            .ok_or(DaemonError::UnexpectedDisconnect)?;
        debug!(?first_msg, "Read raw result from child");

        let initial_info = first_msg.downcast_as_initial_info().map_err(|other| {
            match other.downcast_as_failure() {
                Ok(error) => StartWriterError::Failed(error),
                Err(other) => StartWriterError::UnexpectedFirstStatus(other),
            }
        })?;

        Ok(HerdHandle {
            events: Box::pin(event_rx),
            initial_info,
        })
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
