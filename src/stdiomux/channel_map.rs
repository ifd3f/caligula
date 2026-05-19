use std::{
    cell::RefCell,
    collections::VecDeque,
    fmt::Debug,
    future::poll_fn,
    pin::Pin,
    rc::Rc,
    sync::Arc,
    task::{Context, Poll},
};

use bytes::{Bytes, BytesMut};
use chrono::format::Item;
use futures::{
    FutureExt, Stream, StreamExt as _,
    stream::{self, BoxStream, FusedStream, FuturesUnordered, LocalBoxStream, Peekable},
};
use infinite_stream::{InfiniteStream, StreamExt};
use itertools::Itertools;
use tokio::sync::mpsc;
use tokio_stream::wrappers::UnboundedReceiverStream;

/// Reasonable limit on the number of simultaneous active channels.
const MAX_CHANNELS: u16 = 128;

#[derive(Debug, thiserror::Error)]
#[error("Channel already exists with ID {0}")]
pub struct ChannelExists(u16);

#[derive(Debug, thiserror::Error)]
#[error("Receiver already closed")]
pub struct RecvClosed;

/// Helper for managing a set of channels.
///
/// This object is optimized so that it must be accessed mutably when channels
/// are added or deleted, but may be accessed immutably during steady-state,
pub struct ChannelMap {
    rx_map: Box<[RxState; MAX_CHANNELS as usize]>,
    tx_map: Box<[TxState; MAX_CHANNELS as usize]>,

    /// This value is cached to make linear probing during
    /// [create_new_channel()] more efficient.
    last_inserted_channel_id: u16,
}

pub struct ChannelState<'a> {
    rx: &'a RxState,
    tx: &'a TxState,
}

impl<'a> ChannelState<'a> {
    /// Whether or not this channel is alive.
    ///
    /// A channel is alive if EITHER its RX state OR TX state are alive.
    fn alive(&self) -> bool {
        self.rx.alive() || self.tx.alive()
    }
}

impl ChannelMap {
    pub fn new() -> Self {
        Self {
            rx_map: Box::new([RxState::DEAD; MAX_CHANNELS as usize]),
            tx_map: Box::new([TxState::DEAD; MAX_CHANNELS as usize]),
            last_inserted_channel_id: 0,
        }
    }

    /// Split into RX and TX halves.
    pub fn split<'a>(&'a mut self) -> (ChannelMapRxHalf<'a>, ChannelMapTxHalf<'a>) {
        let rxs = ChannelMapRxHalf {
            rx_map: self.rx_map.as_mut_slice(),
        };
        let txs = ChannelMapTxHalf {
            txs: self
                .tx_map
                .iter_mut()
                .enumerate()
                .filter(|(_id, tx)| tx.alive())
                .map(|(id, tx)| (u16::try_from(id).unwrap(), tx))
                .collect_vec(),
        };
        (rxs, txs)
    }

    /// Attempt to set up a new channel with any ID.
    ///
    /// Returns the channel created along with the stream for receiving inputs
    /// from it.
    pub fn alloc_new_channel(
        &mut self,
        tx_stream: LocalBoxStream<'static, Bytes>,
    ) -> Option<(u16, BoxStream<'static, Bytes>)> {
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
                return self.insert_new_channel(id, tx_stream).ok();
            }
        }
    }

    /// Setup a new channel with a specific ID.
    pub fn insert_new_channel(
        &mut self,
        channel_id: u16,
        tx_stream: LocalBoxStream<'static, Bytes>,
    ) -> Result<(u16, BoxStream<'static, Bytes>), ChannelExists> {
        if self.get_channel(channel_id).alive() {
            return Err(ChannelExists(channel_id));
        }

        // now insert a new value in the free slot
        let idx = channel_id as usize;
        let (rx, tx, rx_stream) = new_channel_state(tx_stream);
        self.rx_map[idx] = rx;
        self.tx_map[idx] = tx;

        // cache this ID to make future calls to alloc_new_channel() faster
        self.last_inserted_channel_id = channel_id;

        Ok((channel_id, rx_stream))
    }

    /// Get the channel state pair with the given ID.
    fn get_channel(&self, id: u16) -> ChannelState<'_> {
        ChannelState {
            rx: self.rx_map.get(id as usize).unwrap_or_default(),
            tx: self.tx_map.get(id as usize).unwrap_or_default(),
        }
    }

    pub fn tx_map_mut(&mut self) -> &mut Box<[TxState; MAX_CHANNELS as usize]> {
        &mut self.tx_map
    }
}

pub struct ChannelMapRxHalf<'a> {
    rx_map: &'a mut [RxState],
}

impl<'a> ChannelMapRxHalf<'a> {
    pub fn handle_rx(&mut self, (id, payload): (u16, Bytes)) -> Result<(), RecvClosed> {
        self.rx_map
            .get_mut(id as usize)
            .ok_or(RecvClosed)?
            .handle_rx(payload)
    }

    pub fn close_rx(&mut self, id: u16) {
        self.rx_map.get_mut(id as usize).map(|s| {
            s.close();
        });
    }
}

pub struct ChannelMapTxHalf<'a> {
    txs: Vec<(u16, &'a mut TxState)>,
}

impl<'a> ChannelMapTxHalf<'a> {
    pub fn tx_stream(self) -> impl InfiniteStream<Item = (u16, Bytes)> {
        let streams = self
            .txs
            .into_iter()
            .map(|(id, tx)| tx.stream().map(move |bs| (id, bs)));
        let merged = stream::select_all(streams);
        merged.chain_pending()
    }
}

/// Create a pair of [RxState] and [TxState].
fn new_channel_state(
    tx_stream: LocalBoxStream<'static, Bytes>,
) -> (RxState, TxState, BoxStream<'static, Bytes>) {
    let (rxq_tx, rxq_rx) = mpsc::unbounded_channel();
    let rx_stream = Box::pin(UnboundedReceiverStream::new(rxq_rx));

    let rx = RxState { rxq: rxq_tx.into() };
    let tx = TxState {
        tx_stream: tx_stream.peekable().into(),
    };

    (rx, tx, rx_stream)
}

#[derive(Debug, Default)]
pub struct RxState {
    rxq: Option<mpsc::UnboundedSender<Bytes>>,
}

impl Default for &RxState {
    fn default() -> Self {
        &RxState::DEAD
    }
}

impl RxState {
    /// A dead variant of this object.
    pub const DEAD: Self = Self { rxq: None };

    /// Whether or not the receive half of this channel is alive.
    pub fn alive(&self) -> bool {
        self.rxq.as_ref().map(|x| !x.is_closed()).unwrap_or(false)
    }

    /// Handle receiving a new value.
    pub fn handle_rx(&mut self, payload: Bytes) -> Result<(), RecvClosed> {
        let rxq = self.rxq.as_ref().ok_or(RecvClosed)?;
        rxq.send(payload).map_err(move |_| RecvClosed)
    }

    pub fn close(&mut self) {
        self.rxq = None;
    }
}

#[derive(Default)]
pub struct TxState {
    tx_stream: Option<Peekable<LocalBoxStream<'static, Bytes>>>,
}

impl Default for &TxState {
    fn default() -> Self {
        &TxState::DEAD
    }
}

impl TxState {
    /// A dead variant of this object.
    pub const DEAD: Self = Self { tx_stream: None };

    /// Whether or not the send half of this channel is alive.
    pub fn alive(&self) -> bool {
        !self.tx_stream.is_some()
    }

    /// Get the stream of values this transmitter wants to send.
    pub fn stream(&mut self) -> impl Stream<Item = Bytes> + '_ {
        stream::iter(&mut self.tx_stream)
            .flatten()
            .filter(|bs| std::future::ready(!bs.is_empty()))
    }
}
