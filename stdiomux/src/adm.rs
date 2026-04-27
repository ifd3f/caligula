use std::{
    collections::{HashMap, VecDeque, hash_map::Entry},
    task::{Context, Poll},
};

use bytes::Bytes;
use futures::{Sink, Stream};
use tokio::sync::{mpsc, oneshot};

use crate::mux::{ChannelId, Frame};

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
            .try_transition_send_syn(our_rx_buffer as u64)?;
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
    SentClose {
        tx_on_closed: oneshot::Sender<()>,
    },
}

impl ChannelState {
    /// Ensures that it's allowable to send a SYN. If it is allowable, returns the SYN to send,
    /// along with a oneshot::Receiver for waiting on channel establishment.
    fn try_transition_send_syn(
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
            | ChannelState::SentClose { .. } => Err(ChannelInUse),
        }
    }

    /// Ensures that it's allowable to send a FIN. If it is allowable, returns the FIN to send,
    /// along with a oneshot::Receiver for waiting on channel closing.
    fn try_transition_send_fin(&mut self) -> Result<(Frame, oneshot::Receiver<()>), AlreadyClosed> {
        match self {
            ChannelState::Closed | ChannelState::SentClose { .. } => Err(AlreadyClosed),
            ChannelState::Established(_)
            | ChannelState::RecvdOpen { .. }
            | ChannelState::SentOpen { .. } => {
                let (tx_on_closed, rx_on_closed) = oneshot::channel();
                *self = ChannelState::SentClose { tx_on_closed };
                Ok((Frame::Fin, rx_on_closed))
            }
        }
    }

    /// Handle receiving a frame. Returns the frame to send in response, if any.
    fn transition_recv(&mut self, frame: Frame) -> Option<Frame> {
        match frame {
            Frame::Reset => {
                self.transition_recv_rst();
                None
            }
            Frame::Data(bytes) => self.transition_recv_data(bytes),
            Frame::Adm(permits) => self.transition_recv_adm(permits),
            Frame::Syn(buf) => self.transition_recv_syn(buf),
            Frame::Fin => self.transition_recv_fin(),
        }
    }

    /// Handle receiving a DAT. Returns the frame to send in response, if any.
    fn transition_recv_data(&mut self, body: Bytes) -> Option<Frame> {
        match self {
            ChannelState::Established(channel_established) => {
                channel_established
                    .rx
                    .tx_to_consumer
                    .try_send(body)
                    .expect("Failed");
                None // Don't send ADM yet -- allow the frames to bunch up before ADM
            }
            ChannelState::SentClose { .. } => None,
            ChannelState::Closed
            | ChannelState::SentOpen { .. }
            | ChannelState::RecvdOpen { .. } => {
                tracing::warn!("Unexpectedly received ADM!");
                *self = ChannelState::Closed;
                Some(Frame::Reset)
            }
        }
    }

    /// Handle receiving a SYN. Returns the frame to send in response, if any.
    fn transition_recv_syn(&mut self, their_rx_buffer: u64) -> Option<Frame> {
        match self {
            ChannelState::Closed | ChannelState::RecvdOpen { .. } => {
                *self = ChannelState::RecvdOpen { their_rx_buffer };
                None
            }
            ChannelState::SentOpen { .. }
            | ChannelState::Established(_)
            | ChannelState::SentClose { .. } => {
                tracing::warn!("Unexpectedly received SYN!");
                *self = ChannelState::Closed;
                Some(Frame::Reset)
            }
        }
    }

    /// Handle receiving a RST (AKA it just closes)
    fn transition_recv_rst(&mut self) {
        *self = ChannelState::Closed;
    }

    /// Handle receiving a FIN
    fn transition_recv_fin(&mut self) -> Option<Frame> {
        match self {
            ChannelState::Established(_)
            | ChannelState::SentOpen { .. }
            | ChannelState::RecvdOpen { .. } => {
                *self = ChannelState::Closed;
                Some(Frame::Fin)
            }
            ChannelState::SentClose { .. } => {
                *self = ChannelState::Closed;
                None
            }
            ChannelState::Closed => {
                tracing::warn!("Unexpectedly received FIN!");
                *self = ChannelState::Closed;
                Some(Frame::Reset)
            }
        }
    }

    /// Handle receiving an ADM
    fn transition_recv_adm(&mut self, permits: u64) -> Option<Frame> {
        match self {
            ChannelState::Established(channel_established) => {
                channel_established.tx.outstanding_permits += permits;
                None
            }
            ChannelState::SentClose { .. } => None,
            ChannelState::Closed
            | ChannelState::SentOpen { .. }
            | ChannelState::RecvdOpen { .. } => {
                tracing::warn!("Unexpectedly received ADM!");
                *self = ChannelState::Closed;
                Some(Frame::Reset)
            }
        }
    }
}

#[derive(Debug, thiserror::Error)]
#[error("Stream ID is already in use")]
pub struct ChannelInUse;

#[derive(Debug, thiserror::Error)]
#[error("Channel already closed")]
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
        let receiver = Receiver {
            rx_from_channel_map,
        };
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
