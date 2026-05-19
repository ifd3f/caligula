use std::cell::RefCell;

use bytes::Bytes;
use futures::Stream;
use tokio::sync::mpsc::{UnboundedSender, unbounded_channel};
use tokio_stream::wrappers::UnboundedReceiverStream;

/// Reasonable limit on the number of simultaneous active channels.
const MAX_CHANNELS: u16 = 128;

#[derive(Debug, thiserror::Error)]
#[error("Channel with ID {0} already exists")]
pub struct ChannelExists(u16);

#[derive(Debug, thiserror::Error)]
#[error("Receiver already closed")]
pub struct RecvClosed;

pub struct ChannelMap {
    inner: RefCell<Inner>,
}

impl ChannelMap {
    pub fn new() -> Self {
        Self {
            inner: Inner::new().into(),
        }
    }

    pub fn handle_rx(&self, id: u16, bs: &Bytes) -> Result<(), RecvClosed> {
        self.inner.borrow().handle_rx(id, bs)
    }

    /// Attempt to set up a new channel with any ID.
    ///
    /// Returns the channel created along with the stream for receiving inputs
    /// from it.
    pub fn alloc_new_channel(
        &self,
    ) -> Option<(u16, impl Stream<Item = Bytes> + Send + use<> + 'static)> {
        self.inner.borrow_mut().alloc_new_channel()
    }

    /// Setup a new channel with a specific ID.
    pub fn insert_new_channel(
        &self,
        channel_id: u16,
    ) -> Result<impl Stream<Item = Bytes> + Send + use<> + 'static, ChannelExists> {
        self.inner.borrow_mut().insert_new_channel(channel_id)
    }

    pub fn close(&self, id: u16) {
        self.inner.borrow_mut().close(id)
    }
}

pub struct Inner {
    rx_map: Box<[RxState; MAX_CHANNELS as usize]>,

    /// This value is cached to make linear probing during
    /// [alloc_new_channel()] more efficient.
    last_inserted_channel_id: u16,
}

impl Inner {
    fn new() -> Self {
        Self {
            rx_map: Box::new([RxState::DEAD; MAX_CHANNELS as usize]),
            last_inserted_channel_id: 0,
        }
    }

    fn handle_rx(&self, id: u16, bs: &Bytes) -> Result<(), RecvClosed> {
        let tx = self.get_channel(id);
        tx.handle_rx(bs)?;
        Ok(())
    }

    /// Attempt to set up a new channel with any ID.
    ///
    /// Returns the channel created along with the stream for receiving inputs
    /// from it.
    fn alloc_new_channel(
        &mut self,
    ) -> Option<(u16, impl Stream<Item = Bytes> + Send + use<> + 'static)> {
        // linearly probe through the ID space until we find a free ID.
        // start from the last inserted value and quit if nothing is alive.
        let mut id = self.last_inserted_channel_id;
        loop {
            id = id.wrapping_add(1) % MAX_CHANNELS;

            if id != self.last_inserted_channel_id {
                // fully looped around. no free slots
                return None;
            }

            if !self.get_channel(id).alive() {
                // it's not living. insert it
                return self.insert_new_channel(id).map(|s| (id, s)).ok();
            }
        }
    }

    /// Setup a new channel with a specific ID.
    fn insert_new_channel(
        &mut self,
        channel_id: u16,
    ) -> Result<impl Stream<Item = Bytes> + Send + use<> + 'static, ChannelExists> {
        if self.get_channel(channel_id).alive() {
            return Err(ChannelExists(channel_id));
        }

        // now insert a new value in the free slot
        let idx = channel_id as usize;
        let (rx, rx_stream) = new_channel_state();
        self.rx_map[idx] = rx;

        // cache this ID to make future calls to alloc_new_channel() faster
        self.last_inserted_channel_id = channel_id;

        Ok(rx_stream)
    }

    /// Get the channel state with the given ID.
    fn get_channel(&self, id: u16) -> &RxState {
        self.rx_map.get(id as usize).unwrap_or(&RxState::DEAD)
    }

    fn close(&mut self, id: u16) {
        let Some(c) = self.rx_map.get_mut(id as usize) else {
            return;
        };
        c.close();
    }
}

/// Create a new [RxState] and its stream.
fn new_channel_state() -> (RxState, impl Stream<Item = Bytes> + Send + 'static) {
    let (rxq_tx, rxq_rx) = unbounded_channel();
    let rx_stream = Box::pin(UnboundedReceiverStream::new(rxq_rx));

    let rx = RxState { rxq: rxq_tx.into() };

    (rx, rx_stream)
}

#[derive(Debug, Default)]
struct RxState {
    rxq: Option<UnboundedSender<Bytes>>,
}

impl RxState {
    /// A dead variant of this object.
    pub const DEAD: Self = Self { rxq: None };

    /// Whether or not the receive half of this channel is alive.
    pub fn alive(&self) -> bool {
        self.rxq.as_ref().map(|x| !x.is_closed()).unwrap_or(false)
    }

    /// Handle receiving a new value.
    pub fn handle_rx(&self, payload: &Bytes) -> Result<(), RecvClosed> {
        let rxq = self.rxq.as_ref().ok_or(RecvClosed)?;
        rxq.send(payload.clone()).map_err(move |_| RecvClosed)
    }

    pub fn close(&mut self) {
        self.rxq = None;
    }
}
