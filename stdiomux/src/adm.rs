use std::{
    collections::{HashMap, VecDeque, hash_map::Entry},
    task::{Context, Poll},
};

use bytes::Bytes;
use futures::{Sink, Stream};
use tokio::sync::{mpsc, oneshot};

use crate::mux::{Frame, StreamId};

pub struct AdmissionController<Rx, Tx>
where
    Rx: Stream<Item = (StreamId, Frame)>,
    Tx: Sink<(StreamId, Frame)>,
{
    inner: std::sync::Mutex<AdmissionControllerInner<Rx, Tx>>,
}

#[derive(Debug, thiserror::Error)]
pub enum OpenStreamError {
    #[error("Connection reset")]
    Reset(#[from] oneshot::error::RecvError),
    #[error("Stream ID already in use")]
    StreamIdInUse(#[from] StreamIdInUse),
}

impl<Rx, Tx> AdmissionController<Rx, Tx>
where
    Rx: Stream<Item = (StreamId, Frame)>,
    Tx: Sink<(StreamId, Frame)>,
{
    pub fn new(rx: Rx, tx: Tx) -> Self {
        let (tx_queue_appender, tx_queue) = mpsc::channel(128);
        let (high_pri_tx_queue_appender, high_pri_tx_queue) = mpsc::unbounded_channel();
        Self {
            inner: std::sync::Mutex::new(AdmissionControllerInner {
                rx,
                tx,
                streams: HashMap::new(),
                tx_queue,
                tx_queue_appender,
                high_pri_tx_queue,
                high_pri_tx_queue_appender,
            }),
        }
    }

    pub async fn open_stream(
        &self,
        stream_id: StreamId,
        initial_rx_buffer: usize,
    ) -> Result<StreamEstablishmentInfo, OpenStreamError> {
        let rx = self
            .inner
            .lock()
            .unwrap()
            .open_stream(stream_id, initial_rx_buffer)?;
        Ok(rx.await?)
    }
}

pub struct Sender {
    tx_to_stream_map: mpsc::Sender<Bytes>,
}

pub struct Receiver {
    rx_from_stream_map: mpsc::Receiver<Bytes>,
}

struct AdmissionControllerInner<Rx, Tx>
where
    Rx: Stream<Item = (StreamId, Frame)>,
    Tx: Sink<(StreamId, Frame)>,
{
    rx: Rx,
    tx: Tx,
    /// Active, non-closed streams.
    streams: HashMap<StreamId, StreamMapEntry>,
    /// Queue of frames to transmit.
    tx_queue: mpsc::Receiver<(StreamId, Frame)>,
    /// Queue of frames to transmit.
    tx_queue_appender: mpsc::Sender<(StreamId, Frame)>,
    /// Queue of frames to transmit.
    high_pri_tx_queue: mpsc::UnboundedReceiver<(StreamId, Frame)>,
    /// Queue of frames to transmit.
    high_pri_tx_queue_appender: mpsc::UnboundedSender<(StreamId, Frame)>,
}

impl<Rx, Tx> AdmissionControllerInner<Rx, Tx>
where
    Rx: Stream<Item = (StreamId, Frame)>,
    Tx: Sink<(StreamId, Frame)>,
{
    pub fn open_stream(
        &mut self,
        stream_id: StreamId,
        our_rx_buffer: usize,
    ) -> Result<oneshot::Receiver<StreamEstablishmentInfo>, StreamIdInUse> {
        let (frame, out) = self
            .streams
            .entry(stream_id)
            .or_insert_with(|| StreamMapEntry {
                state: State::Closed,
            })
            .state
            .transition_send_syn(our_rx_buffer as u64)?;
        self.high_pri_tx_queue_appender
            .send((stream_id, frame))
            .unwrap();
        Ok(out)
    }
}

struct StreamMapEntry {
    /// current state of the connection
    state: State,
}

impl StreamMapEntry {}

#[derive(Default)]
enum State {
    #[default]
    Closed,
    SentOpen {
        tx_on_established: oneshot::Sender<StreamEstablishmentInfo>,
        our_rx_buffer: u64,
    },
    RecvdOpen {
        their_rx_buffer: u64,
    },
    Established(StreamEstablished),
    SentClose,
    RecvdClose,
}

impl State {
    /// Ensures that it's allowable to send a SYN. If it is allowable, returns the SYN to send,
    /// along with a oneshot::Receiver for waiting on stream establishment.
    fn transition_send_syn(
        &mut self,
        our_rx_buffer: u64,
    ) -> Result<(Frame, oneshot::Receiver<StreamEstablishmentInfo>), StreamIdInUse> {
        let syn = Frame::Syn(our_rx_buffer as u64);
        let (tx_on_established, rx_on_established) = oneshot::channel();
        match self {
            State::Closed => {
                *self = State::SentOpen {
                    tx_on_established,
                    our_rx_buffer,
                };
                Ok((syn, rx_on_established))
            }
            State::RecvdOpen { their_rx_buffer } => {
                let (est, info) =
                    StreamEstablished::new(our_rx_buffer as usize, *their_rx_buffer as usize);

                *self = State::Established(est);

                let (tx_on_established, rx_on_established) = oneshot::channel();
                tx_on_established.send(info).map_err(|_| ()).unwrap(); // impossible to fail

                Ok((syn, rx_on_established))
            }
            State::SentOpen { .. }
            | State::Established(_)
            | State::SentClose
            | State::RecvdClose => Err(StreamIdInUse),
        }
    }
}

#[derive(Debug, thiserror::Error)]
#[error("Stream ID is already in use")]
struct StreamIdInUse;

#[derive(Debug, thiserror::Error)]
#[error("oops already closed")]
struct AlreadyClosed;

pub struct StreamEstablishmentInfo {
    pub tx: Sender,
    pub rx: Receiver,
}

struct StreamEstablished {
    tx: StreamTxState,
    rx: StreamRxState,
}

impl StreamEstablished {
    fn new(rx_buffer: usize, tx_buffer: usize) -> (StreamEstablished, StreamEstablishmentInfo) {
        let (tx_to_stream_map, rx_from_consumer) = mpsc::channel(tx_buffer);
        let (tx_to_consumer, rx_from_stream_map) = mpsc::channel(rx_buffer);
        let sender = Sender { tx_to_stream_map };
        let receiver = Receiver { rx_from_stream_map };
        (
            StreamEstablished {
                tx: StreamTxState {
                    rx_from_consumer,
                    outstanding_permits: 0,
                },
                rx: StreamRxState { tx_to_consumer },
            },
            StreamEstablishmentInfo {
                tx: sender,
                rx: receiver,
            },
        )
    }
}

struct StreamTxState {
    /// rx for receiving payloads from the consumer
    rx_from_consumer: mpsc::Receiver<Bytes>,

    /// number of unconsumed send permits the receiver has granted us
    outstanding_permits: u64,
}

struct StreamRxState {
    /// tx for sending payloads to the consumer
    tx_to_consumer: mpsc::Sender<Bytes>,
}
