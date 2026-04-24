use crate::{
    bincode_options,
    mux::{DatagramHandler, MuxError},
};
use bincode::Options;
use bytes::Bytes;
use futures::{Sink, SinkExt, future::BoxFuture};
use serde::{Serialize, de::DeserializeOwned};
use std::{collections::HashMap, marker::PhantomData, sync::Arc, task::Poll};
use tokio::sync::oneshot;
use tower_service::Service;
use tracing::warn;

#[derive(Debug, thiserror::Error)]
pub enum RpcError<Err> {
    #[error("Application error: {0}")]
    Application(Err),
    #[error("Serialization failure: {0}")]
    Serialization(#[from] bincode::Error),
    #[error("Transport failure: {0}")]
    Transport(#[from] Arc<MuxError>),
    #[error("Unexpectedly disconnected")]
    Disconnected,
}

pub struct RpcClient<Tx, Req, Res, Err>
where
    Req: Serialize + Send + 'static,
    Res: DeserializeOwned + Send + Clone + 'static,
    Err: DeserializeOwned + Send + Sync + 'static,
    Tx: Sink<Bytes, Error = Arc<MuxError>> + Unpin + Send + 'static,
{
    tx: Arc<tokio::sync::Mutex<Tx>>,
    map: std::sync::Mutex<Option<RequestMap<Result<Res, Arc<RpcError<Err>>>>>>,
    _phantom: PhantomData<Req>,
}

impl<Tx, Req, Res, Err> RpcClient<Tx, Req, Res, Err>
where
    Req: Serialize + Send + 'static,
    Res: DeserializeOwned + Send + Clone + 'static,
    Err: DeserializeOwned + Send + Sync + 'static,
    Tx: Sink<Bytes, Error = Arc<MuxError>> + Unpin + Send + 'static,
{
    pub fn new(tx: Arc<tokio::sync::Mutex<Tx>>) -> Self {
        Self {
            tx,
            map: std::sync::Mutex::new(Some(RequestMap::new())),
            _phantom: PhantomData,
        }
    }
}

impl<Tx, Req, Res, Err> DatagramHandler for RpcClient<Tx, Req, Res, Err>
where
    Req: Serialize + Send + 'static,
    Res: DeserializeOwned + Send + Clone + 'static,
    Err: DeserializeOwned + Send + Sync + 'static,
    Tx: Sink<Bytes, Error = Arc<MuxError>> + Unpin + Send + 'static,
{
    fn handle_datagram(&self, res: Result<Bytes, Arc<MuxError>>) -> std::ops::ControlFlow<()> {
        let mut lock = self.map.lock().unwrap();
        let Some(mut map) = lock.take() else {
            return std::ops::ControlFlow::Break(());
        };
        // Handle result
        let bs = match res {
            Ok(bs) => bs,
            Err(e) => {
                // Broadcast error to everything
                map.close_with_global_result(Err(Arc::new(RpcError::Transport(e))));
                return std::ops::ControlFlow::Break(());
            }
        };
        // Decode and send to the one request
        match bincode_options().deserialize(&bs) {
            Ok((request_id, res)) => match map.demux_map.remove(&request_id) {
                Some(tx) => {
                    tx.send(Ok(res)).ok();
                }
                None => {
                    warn!("Received response to nonexistent request {request_id}");
                }
            },
            Err(e) => {
                warn!("Failed to deserialize response from server: {e}");
            }
        }
        *lock = Some(map);
        std::ops::ControlFlow::Continue(())
    }
}

impl<Tx, Req, Res, Err> Service<Req> for RpcClient<Tx, Req, Res, Err>
where
    Req: Serialize + Send + 'static,
    Res: DeserializeOwned + Send + Clone + 'static,
    Err: DeserializeOwned + Send + Sync + 'static,
    Tx: Sink<Bytes, Error = Arc<MuxError>> + Unpin + Send + 'static,
{
    type Response = Res;
    type Error = Arc<RpcError<Err>>;
    type Future =
        BoxFuture<'static, Result<<Self as Service<Req>>::Response, <Self as Service<Req>>::Error>>;

    fn poll_ready(
        &mut self,
        _cx: &mut std::task::Context<'_>,
    ) -> Poll<Result<(), <Self as Service<Req>>::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: Req) -> <Self as Service<Req>>::Future {
        // Allocate a slot in the request map
        let mut lock = self.map.lock().unwrap();
        let Some(map) = lock.as_mut() else {
            // No map means we got disconnected
            return Box::pin(std::future::ready(Err(Arc::new(RpcError::Disconnected))));
        };
        let (request_id, rx) = map.allocate_request_slot();
        drop(lock);
        // Attempt to serialize the request
        let bs = match bincode_options().serialize(&(request_id, req)) {
            Ok(bs) => Bytes::from(bs),
            Err(e) => return Box::pin(std::future::ready(Err(Arc::new(e.into())))),
        };
        // That was all relatively fast sync code! Time for the async!
        let tx = self.tx.clone();
        Box::pin(async move {
            // Actually perform the send
            let mut tx = tx.lock().await;
            tx.send(bs).await.map_err(|e| RpcError::Transport(e))?;
            drop(tx);
            // Receive the result from the map and convert it into the user type
            let rx_result: Result<Res, Arc<RpcError<Err>>> = rx
                .await
                .map_err(|_| Arc::new(RpcError::<Err>::Disconnected))?;
            rx_result
        })
    }
}

struct RequestMap<T> {
    next_id: u64,
    demux_map: HashMap<u64, oneshot::Sender<T>>,
}

impl<T: Clone> RequestMap<T> {
    fn new() -> Self {
        Self {
            next_id: 0,
            demux_map: HashMap::new(),
        }
    }

    fn allocate_request_slot(&mut self) -> (u64, oneshot::Receiver<T>) {
        let id = self.next_id;
        self.next_id += self.next_id.wrapping_add(1);
        let (tx, rx) = oneshot::channel();
        self.demux_map.insert(id, tx);
        (id, rx)
    }

    fn close_with_global_result(self, x: T) {
        for (_, tx) in self.demux_map {
            tx.send(x.clone()).ok();
        }
    }
}
