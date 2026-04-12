use std::collections::HashMap;
use std::hash::Hash;
use std::sync::Arc;

use futures::stream::{self, BoxStream};
use tokio::sync::mpsc::{self, error::TryRecvError};

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
