use std::{
    collections::{HashMap, VecDeque, hash_map::Entry},
    task::{Context, Poll},
};

use bytes::Bytes;
use futures::{Sink, Stream};
use tokio::sync::{mpsc, oneshot};

use crate::mux::{Frame, ChannelId as ChannelId};

pub struct AdmissionController<Rx, Tx>
where
    Rx: Stream<Item = (ChannelId, Frame)>,
    Tx: Sink<(ChannelId, Frame)>,
{
    inner: std::sync::Mutex<AdmissionControllerInner<Rx, Tx>>,
}

#[derive(Debug, thiserror::Error)]
pub enum OpenChannelError {
    #[error("Connection reset")]
    Reset(#[from] oneshot::error::RecvError),
    #[error("Channel already in use")]
    ChannelInUse(#[from] ChannelInUse),
}

impl<Rx, Tx> AdmissionController<Rx, Tx>
where
    Rx: Stream<Item = (ChannelId, Frame)>,
    Tx: Sink<(ChannelId, Frame)>,
{
    pub fn new(rx: Rx, tx: Tx) -> Self {
        let (tx_queue_appender, tx_queue) = mpsc::channel(128);
        let (high_pri_tx_queue_appender, high_pri_tx_queue) = mpsc::unbounded_channel();
        Self {
            inner: std::sync::Mutex::new(AdmissionControllerInner {
                rx,
                tx,
                channels: HashMap::new(),
                tx_queue,
                tx_queue_appender,
                high_pri_tx_queue,
                high_pri_tx_queue_appender,
            }),
        }
    }

    pub async fn open_channel(
        &self,
        channel_id: ChannelId,
        initial_rx_buffer: usize,
    ) -> Result<ChannelEstablishmentInfo, OpenChannelError> {
        let rx = self
            .inner
            .lock()
            .unwrap()
            .open_channel(channel_id, initial_rx_buffer)?;
        Ok(rx.await?)
    }
}

pub struct Sender {
    tx_to_channel_map: mpsc::Sender<Bytes>,
}

pub struct Receiver {
    rx_from_channel_map: mpsc::Receiver<Bytes>,
}

struct AdmissionControllerInner<Rx, Tx>
where
    Rx: Stream<Item = (ChannelId, Frame)>,
    Tx: Sink<(ChannelId, Frame)>,
{
    rx: Rx,
    tx: Tx,
    /// Active, non-closed channels.
    channels: HashMap<ChannelId, ChannelMapEntry>,
    /// Queue of frames to transmit.
    tx_queue: mpsc::Receiver<(ChannelId, Frame)>,
    /// Queue of frames to transmit.
    tx_queue_appender: mpsc::Sender<(ChannelId, Frame)>,
    /// Queue of frames to transmit.
    high_pri_tx_queue: mpsc::UnboundedReceiver<(ChannelId, Frame)>,
    /// Queue of frames to transmit.
    high_pri_tx_queue_appender: mpsc::UnboundedSender<(ChannelId, Frame)>,
}

impl<Rx, Tx> AdmissionControllerInner<Rx, Tx>
where
    Rx: Stream<Item = (ChannelId, Frame)>,
    Tx: Sink<(ChannelId, Frame)>,
{
    pub fn open_channel(
        &mut self,
        channel_id: ChannelId,
        our_rx_buffer: usize,
    ) -> Result<oneshot::Receiver<ChannelEstablishmentInfo>, ChannelInUse> {
        let (frame, out) = self
            .channels
            .entry(channel_id)
            .or_insert_with(|| ChannelMapEntry {
                state: ChannelState::Closed,
            })
            .state
            .transition_send_syn(our_rx_buffer as u64)?;
        self.high_pri_tx_queue_appender
            .send((channel_id, frame))
            .unwrap();
        Ok(out)
    }
}

struct ChannelMapEntry {
    /// current state of the connection
    state: ChannelState,
}

impl ChannelMapEntry {}

#[derive(Default)]
enum ChannelState {
    #[default]
    Closed,
    SentOpen {
        tx_on_established: oneshot::Sender<ChannelEstablishmentInfo>,
        our_rx_buffer: u64,
    },
    RecvdOpen {
        their_rx_buffer: u64,
    },
    Established(ChannelEstablished),
    SentClose,
    RecvdClose,
}

impl ChannelState {
    /// Ensures that it's allowable to send a SYN. If it is allowable, returns the SYN to send,
    /// along with a oneshot::Receiver for waiting on channel establishment.
    fn transition_send_syn(
        &mut self,
        our_rx_buffer: u64,
    ) -> Result<(Frame, oneshot::Receiver<ChannelEstablishmentInfo>), ChannelInUse> {
        let syn = Frame::Syn(our_rx_buffer as u64);
        let (tx_on_established, rx_on_established) = oneshot::channel();
        match self {
            ChannelState::Closed => {
                *self = ChannelState::SentOpen {
                    tx_on_established,
                    our_rx_buffer,
                };
                Ok((syn, rx_on_established))
            }
            ChannelState::RecvdOpen { their_rx_buffer } => {
                let (est, info) =
                    ChannelEstablished::new(our_rx_buffer as usize, *their_rx_buffer as usize);

                *self = ChannelState::Established(est);

                let (tx_on_established, rx_on_established) = oneshot::channel();
                tx_on_established.send(info).map_err(|_| ()).unwrap(); // impossible to fail

                Ok((syn, rx_on_established))
            }
            ChannelState::SentOpen { .. }
            | ChannelState::Established(_)
            | ChannelState::SentClose
            | ChannelState::RecvdClose => Err(ChannelInUse),
        }
    }
}

#[derive(Debug, thiserror::Error)]
#[error("Stream ID is already in use")]
pub struct ChannelInUse;

#[derive(Debug, thiserror::Error)]
#[error("oops already closed")]
struct AlreadyClosed;

pub struct ChannelEstablishmentInfo {
    pub tx: Sender,
    pub rx: Receiver,
}

struct ChannelEstablished {
    tx: ChannelTxState,
    rx: ChannelRxState,
}

impl ChannelEstablished {
    fn new(rx_buffer: usize, tx_buffer: usize) -> (ChannelEstablished, ChannelEstablishmentInfo) {
        let (tx_to_channel_map, rx_from_consumer) = mpsc::channel(tx_buffer);
        let (tx_to_consumer, rx_from_channel_map) = mpsc::channel(rx_buffer);
        let sender = Sender { tx_to_channel_map };
        let receiver = Receiver { rx_from_channel_map };
        (
            ChannelEstablished {
                tx: ChannelTxState {
                    rx_from_consumer,
                    outstanding_permits: 0,
                },
                rx: ChannelRxState { tx_to_consumer },
            },
            ChannelEstablishmentInfo {
                tx: sender,
                rx: receiver,
            },
        )
    }
}

struct ChannelTxState {
    /// rx for receiving payloads from the consumer
    rx_from_consumer: mpsc::Receiver<Bytes>,

    /// number of unconsumed send permits the receiver has granted us
    outstanding_permits: u64,
}

struct ChannelRxState {
    /// tx for sending payloads to the consumer
    tx_to_consumer: mpsc::Sender<Bytes>,
}
