use std::marker::PhantomData;

use bytes::Bytes;
use dashmap::DashMap;
use futures::{Sink, SinkExt, StreamExt, TryStream};
use std::fmt::Debug;
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot, watch};

use crate::mux::{ChannelId, Frame};

pub struct AdmissionController<Rx, Tx>
where
    Rx: TryStream<Item = (ChannelId, Frame)> + Unpin,
    Tx: Sink<(ChannelId, Frame)> + Unpin,
{
    inner: Arc<AdmissionControllerInner>,
    /// Queue of frames to transmit.
    tx_queue_appender: mpsc::Sender<(ChannelId, Frame)>,
    /// Queue of frames to transmit.
    high_pri_tx_queue_appender: mpsc::UnboundedSender<(ChannelId, Frame)>,
    _phantom: PhantomData<(Rx, Tx)>,
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
    Rx: TryStream<Item = (ChannelId, Frame)> + Unpin + Send + 'static,
    Tx: Sink<(ChannelId, Frame)> + Unpin + Send + 'static,
    <Tx as futures::Sink<(ChannelId, Frame)>>::Error: Debug,
{
    pub fn new(mut rx: Rx, mut tx: Tx) -> Self {
        let (tx_queue_appender, mut tx_queue) = mpsc::channel(128);
        let (high_pri_tx_queue_appender, mut high_pri_tx_queue) = mpsc::unbounded_channel();
        let inner_value = Arc::new(AdmissionControllerInner {
            channels: DashMap::new(),
        });

        let _rx_actor = tokio::spawn({
            let inner = inner_value.clone();
            let high_pri_tx_queue_appender = high_pri_tx_queue_appender.clone();
            async move {
                while let Some((channel_id, frame)) = rx.next().await {
                    // try to forward the message to the actor
                    let Some(entry) = inner.channels.get(&channel_id) else {
                        high_pri_tx_queue_appender
                            .send((channel_id, Frame::Reset))
                            .unwrap();
                        return;
                    };
                    let send = entry.handle_frame(frame);

                    if let Some(send) = send {
                        let Ok(()) = high_pri_tx_queue_appender.send((channel_id, send)) else {
                            todo!()
                        };
                    }
                }
            }
        });

        let _tx_actor = tokio::spawn(async move {
            loop {
                tokio::select! {
                    biased;
                    v = high_pri_tx_queue.recv() => {
                        let Some(v) = v else { return; };
                        tx.send(v).await.unwrap()
                    }
                    v = tx_queue.recv() => {
                        let Some(v) = v else { return; };
                        tx.send(v).await.unwrap()
                    }
                }
            }
        });

        Self {
            inner: inner_value,
            tx_queue_appender,
            high_pri_tx_queue_appender,
            _phantom: PhantomData,
        }
    }

    pub async fn open_channel(
        &self,
        channel_id: ChannelId,
        initial_rx_buffer: usize,
    ) -> Result<ChannelIo, OpenChannelError> {
        let (frame, rx) = self.inner.open_channel(channel_id, initial_rx_buffer)?;
        let Ok(()) = self.high_pri_tx_queue_appender.send((channel_id, frame)) else {
            todo!()
        };

        let tx_queue_appender = self.tx_queue_appender.clone();
        let (mut actor_wires, io) = rx.await?;

        let _channel_tx_actor = tokio::spawn(async move {
            let mut next_seqno = 0u64;

            loop {
                // grab send from user
                let next_tx = actor_wires.rx_from_user.recv();

                // wait until max_seqno allows for this send to happen
                let adm_enough = async {
                    actor_wires
                        .rx_max_seqno
                        .wait_for(|max_seqno| *max_seqno >= next_seqno)
                        .await?;
                    Ok::<_, watch::error::RecvError>(())
                };

                let x = tokio::join!(next_tx, adm_enough);
                match x {
                    // if either of these is disconnected, then we're done
                    (None, _) | (_, Err(_)) => break,

                    // send the packet over
                    (Some(bs), Ok(_)) => {
                        let Ok(()) = tx_queue_appender.send((channel_id, Frame::Data(bs))).await
                        else {
                            break;
                        };
                    }
                }

                // increment seqno
                next_seqno += 1;
            }
        });

        Ok(io)
    }
}

pub struct Sender {
    tx_to_actor: mpsc::Sender<Bytes>,
}

pub struct Receiver {
    rx_from_channel_map: mpsc::Receiver<Bytes>,
}

struct AdmissionControllerInner {
    /// Active, non-closed channels.
    channels: DashMap<ChannelId, ChannelMapEntry>,
}

impl AdmissionControllerInner {
    fn open_channel(
        &self,
        channel_id: ChannelId,
        our_rx_buffer: usize,
    ) -> Result<(Frame, oneshot::Receiver<(ChannelTxActorWires, ChannelIo)>), ChannelInUse> {
        self.channels
            .entry(channel_id)
            .or_insert_with(|| ChannelMapEntry {
                state: std::sync::Mutex::new(ChannelState::Closed),
            })
            .state
            .lock()
            .unwrap()
            .try_transition_send_syn(our_rx_buffer as u64)
    }
}

struct ChannelMapEntry {
    /// current state of the connection
    state: std::sync::Mutex<ChannelState>,
}

impl ChannelMapEntry {
    fn handle_frame(&self, frame: Frame) -> Option<Frame> {
        self.state.lock().unwrap().transition_recv(frame)
    }
}

#[derive(Default)]
enum ChannelState {
    #[default]
    Closed,
    SentOpen {
        tx_on_established: oneshot::Sender<(ChannelTxActorWires, ChannelIo)>,
        our_rx_buffer: u64,
    },
    RecvdOpen {
        their_rx_buffer: u64,
    },
    Established(EstablishedChannelEntry),
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
    ) -> Result<(Frame, oneshot::Receiver<(ChannelTxActorWires, ChannelIo)>), ChannelInUse> {
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
                let (entry, actor, user) =
                    setup_established_channel_wires(our_rx_buffer as u64, *their_rx_buffer as u64);

                *self = ChannelState::Established(entry);

                let (tx_on_established, rx_on_established) = oneshot::channel();
                tx_on_established
                    .send((actor, user))
                    .map_err(|_| ())
                    .unwrap(); // impossible to fail

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
                    .tx_to_user
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
    fn transition_recv_adm(&mut self, max_seqno: u64) -> Option<Frame> {
        match self {
            ChannelState::Established(channel_established) => {
                channel_established.tx_max_seqno.send(max_seqno).ok();
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

pub struct ChannelIo {
    tx: Sender,
    rx: Receiver,
}

struct EstablishedChannelEntry {
    tx_max_seqno: watch::Sender<u64>,
    tx_to_user: mpsc::Sender<Bytes>,
}

struct ChannelTxActorWires {
    rx_max_seqno: watch::Receiver<u64>,
    rx_from_user: mpsc::Receiver<Bytes>,
}

fn setup_established_channel_wires(
    our_rx_buffer: u64,
    their_rx_buffer: u64,
) -> (EstablishedChannelEntry, ChannelTxActorWires, ChannelIo) {
    let (tx_to_actor, rx_from_user) = mpsc::channel(their_rx_buffer as usize);
    let (tx_to_user, rx_from_channel_map) = mpsc::channel(our_rx_buffer as usize);
    let (tx_max_seqno, rx_max_seqno) = watch::channel(their_rx_buffer);

    let sender = Sender { tx_to_actor };
    let receiver = Receiver {
        rx_from_channel_map,
    };

    (
        EstablishedChannelEntry {
            tx_max_seqno,
            tx_to_user,
        },
        ChannelTxActorWires {
            rx_max_seqno,
            rx_from_user,
        },
        ChannelIo {
            tx: sender,
            rx: receiver,
        },
    )
}
