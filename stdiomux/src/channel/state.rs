use std::{
    fmt::Debug,
    num::NonZero,
    task::{Context, Poll},
};

use bytes::Bytes;

use crate::frame::{ChannelControlHeader, ChannelDataFrame};

#[derive(Default)]
pub enum ChannelState<B: ChannelBuffer> {
    #[default]
    Closed,
    RecvOpen,
    SendOpen {
        buf: Box<dyn FnOnce() -> B + 'static>,
    },
    Open(OpenChannel<B>),
}

impl<B: ChannelBuffer> Debug for ChannelState<B> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Closed => write!(f, "Closed"),
            Self::RecvOpen => write!(f, "RecvOpen"),
            Self::SendOpen { .. } => f.debug_struct("SendOpen").finish(),
            Self::Open(o) => f.debug_tuple("Open").field(o).finish(),
        }
    }
}

#[derive(Default, PartialEq, Eq)]
pub struct OpenChannel<B: ChannelBuffer> {
    their_available_permits: u64,
    our_available_permits: u64,
    buf: B,
}

impl<B: ChannelBuffer> Debug for OpenChannel<B> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OpenChannel")
            .field("their_available_permits", &self.their_available_permits)
            .field("our_available_permits", &self.our_available_permits)
            .finish()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum AcceptRxError {
    #[error("Buffer is out of capacity")]
    OutOfCapacity,
    #[error("Buffer is disconnected")]
    Disconnected,
}

/// Interface for the channel state machine to receive and place data.
pub trait ChannelBuffer {
    /// Poll for how many payloads can be received on this channel.
    ///
    /// Returns the amount of capacity, [`Poll::Pending`] if no capacity,
    /// or [`None`] if this buffer is closed.
    fn poll_rx_capacity(&mut self, cx: &mut Context<'_>) -> Poll<Option<NonZero<u64>>>;

    /// Place received data into this channel.
    ///
    /// Returning an error will cause the buffer to disconnect.
    ///
    /// If `poll_rx_capacity()` is implemented correctly, this is guaranteed to always work.
    /// As a failsafe, if it turns out there is no more capacity, return [`AcceptRxError::OutOfCapacity`].
    fn accept_rx(&mut self, data: Bytes) -> Result<(), AcceptRxError>;

    /// Poll data to transmit on this buffer.
    fn poll_tx(&mut self, cx: &mut Context<'_>) -> Poll<Bytes>;
}

impl<B: ChannelBuffer> ChannelState<B> {
    pub fn on_recv_data(self, data: ChannelDataFrame) -> (Self, Option<ChannelControlHeader>) {
        let close = (Self::Closed, Some(ChannelControlHeader::Reset));

        match self {
            // If we get data outside of Open, close the stream
            Self::Closed | Self::RecvOpen | Self::SendOpen { .. } => close,

            // Handle the data
            Self::Open(mut o) => {
                // ensure that they have enough permits first
                let Some(new_permits) = o.their_available_permits.checked_sub(1) else {
                    return close;
                };
                o.their_available_permits = new_permits;

                // attempt to accept the rx
                let Ok(_) = o.buf.accept_rx(data.0) else {
                    return close;
                };
                (Self::Open(o), None)
            }
        }
    }

    pub fn on_recv_control(self, f: ChannelControlHeader) -> (Self, Option<ChannelControlHeader>) {
        use ChannelControlHeader::*;

        let send_close = (Self::Closed, Some(Reset));

        match (self, f) {
            // if got reset, go into closed
            (_, Reset) => (Self::Closed, None),

            // transition into recvopen
            (Self::Closed, Open) => (Self::RecvOpen, None),

            // combine our sendopen and the existing open into an opened channel
            (Self::SendOpen { buf }, Open) => {
                let buf = buf();
                (
                    Self::Open(OpenChannel {
                        their_available_permits: 0,
                        our_available_permits: 0,
                        buf,
                    }),
                    None,
                )
            }

            // can't get opens in any other state
            (_, Open) => send_close,

            // got admit in Open
            (Self::Open(mut o), Admit(permits)) => {
                o.our_available_permits += permits as u64;
                (Self::Open(o), None)
            }

            // can't get admits outside of Open
            (_, Admit(_)) => send_close,
        }
    }
}
