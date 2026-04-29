use std::{
    fmt::{Debug, DebugList},
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

#[derive(Default)]
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
/// Interface for interacting with a ChannelBuffer that may or may not be alive.
pub trait ChannelBuffer {
    type Live: LiveChannelBuffer;

    /// Ensures that this buffer is alive.
    fn ensure_alive(&mut self) -> Option<&mut Self::Live>;

    /// Attempt to accept a piece of data that has been received. This method must either
    /// return the live version, or return None to signal that it cannot place the data
    /// and must therefore close the stream.
    fn accept_rx(&mut self, data: Bytes) -> Option<&mut Self::Live>;
}

/// Interface for interacting with a channel buffer that is guaranteed to be alive.
pub trait LiveChannelBuffer {
    /// Called when new transmission permits are granted.
    fn on_grant_tx_permits(&mut self, permits: u8);

    /// Returns how much capacity is available for receiving on this channel.
    fn rx_capacity(&mut self) -> u64;

    /// Poll for new data to transmit on this buffer.
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
                let Some(_) = o.buf.accept_rx(data.0) else {
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
