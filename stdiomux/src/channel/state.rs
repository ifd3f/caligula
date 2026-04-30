use std::{
    fmt::Debug,
    num::NonZero,
    task::{Context, Poll, Waker},
};

use bytes::Bytes;

use crate::{
    frame::{ChannelControlHeader, ChannelDataFrame},
    util::panic_or_warn,
};

#[derive(Default)]
pub enum ChannelState<B: ChannelBuffer> {
    #[default]
    Closed,
    RecvOpen,
    SendOpen {
        buf: B,
        waker: Option<Waker>,
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
    tx_waker: Option<Waker>,
    rx_waker: Option<Waker>,
}

impl<B: ChannelBuffer> OpenChannel<B> {
    pub fn new(buf: B) -> Self {
        Self {
            their_available_permits: 0,
            our_available_permits: 0,
            buf,
            tx_waker: None,
            rx_waker: None,
        }
    }
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

#[derive(Debug, thiserror::Error)]
#[error("Channel is in use, and already opened by another caller")]
pub struct ChannelInUse;

/// Interface for the channel state machine to receive and place data.
pub trait ChannelBuffer: Default {
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
    fn poll_tx(&mut self, cx: &mut Context<'_>) -> Poll<Option<Bytes>>;
}

#[cfg(test)]
#[derive(Debug, Default)]
pub struct NullChannelBuffer;

#[cfg(test)]
impl ChannelBuffer for NullChannelBuffer {
    fn poll_rx_capacity(&mut self, _cx: &mut Context<'_>) -> Poll<Option<NonZero<u64>>> {
        Poll::Ready(Some(NonZero::try_from(100).unwrap()))
    }

    fn accept_rx(&mut self, _data: Bytes) -> Result<(), AcceptRxError> {
        Ok(())
    }

    fn poll_tx(&mut self, _cx: &mut Context<'_>) -> Poll<Option<Bytes>> {
        Poll::Pending
    }
}

impl<B: ChannelBuffer> ChannelState<B> {
    pub fn poll_open(
        &mut self,
        cx: &mut Context<'_>,
        make_buf: impl FnOnce() -> B,
    ) -> Poll<Result<(), ChannelInUse>> {
        match self {
            ChannelState::Closed => {
                *self = ChannelState::SendOpen {
                    buf: make_buf(),
                    waker: Some(cx.waker().clone()),
                };
                Poll::Pending
            }
            ChannelState::RecvOpen => {
                *self = ChannelState::Open(OpenChannel::new(make_buf()));
                Poll::Ready(Ok(()))
            }
            ChannelState::SendOpen { .. } | ChannelState::Open(_) => Poll::Ready(Err(ChannelInUse)),
        }
    }

    pub fn on_recv_data(&mut self, data: ChannelDataFrame) -> Option<ChannelControlHeader> {
        let Self::Open(o) = self else {
            return None;
        };

        // ensure that they have enough permits first
        let Some(new_permits) = o.their_available_permits.checked_sub(1) else {
            // not enough permits -- close the channel out of safety
            panic_or_warn!(
                "Channel buffer out of capacity! This may be a logic error on the other side!"
            );
            *self = ChannelState::Closed;
            return Some(ChannelControlHeader::Reset);
        };

        o.their_available_permits = new_permits;
        if let Some(w) = o.rx_waker.take() {
            w.wake();
        }

        // attempt to accept the rx
        match o.buf.accept_rx(data.0) {
            Ok(_) => (),
            Err(AcceptRxError::OutOfCapacity) => {
                panic_or_warn!(
                    "Channel buffer out of capacity! This may be a logic error in the buffer implementation!"
                );
                *self = ChannelState::Closed;
                return Some(ChannelControlHeader::Reset);
            }
            Err(AcceptRxError::Disconnected) => {
                *self = ChannelState::Closed;
                return Some(ChannelControlHeader::Reset);
            }
        }

        None
    }

    /// Calculate if we need to send an ADM packet
    #[inline]
    pub fn poll_next_adm(&mut self, cx: &mut Context<'_>) -> Poll<ChannelControlHeader> {
        let Self::Open(o) = self else {
            return Poll::Pending;
        };

        let actual_capacity = match o.buf.poll_rx_capacity(cx) {
            // we have capacity
            Poll::Ready(Some(x)) => x.into(),

            // buffer has closed
            Poll::Ready(None) => {
                *self = ChannelState::Closed;
                return Poll::Ready(ChannelControlHeader::Reset);
            }

            // we are waiting for more capacity
            Poll::Pending => 0,
        };

        match actual_capacity.cmp(&o.their_available_permits) {
            std::cmp::Ordering::Less => {
                panic_or_warn!(
                    "actual_capacity {} is less than their_available_permits {}! This may be a logic error in the buffer implementation",
                    actual_capacity,
                    o.their_available_permits
                );

                // if can't panic, quit to be safe
                *self = ChannelState::Closed;
                return Poll::Ready(ChannelControlHeader::Reset);
            }

            // capacity has not changed, so no adm is needed
            std::cmp::Ordering::Equal => Poll::Pending,

            // capacity has increased
            std::cmp::Ordering::Greater => {
                // calculate how many additional permits we need to send
                let needed_additional_permits = actual_capacity - o.their_available_permits;

                // we can only send 255 permits at a time, so clampingly convert
                let actual_additional_permits =
                    u8::try_from(needed_additional_permits).unwrap_or(u8::MAX);

                // increment our count of their permits
                o.their_available_permits += actual_additional_permits as u64;
                Poll::Ready(ChannelControlHeader::Admit(actual_additional_permits))
            }
        }
    }

    /// Calculate if we can send data
    #[inline]
    pub fn poll_next_tx(&mut self, cx: &mut Context<'_>) -> Poll<Option<ChannelDataFrame>> {
        let Self::Open(o) = self else {
            return Poll::Ready(None);
        };

        // out of permits -- don't send
        if o.our_available_permits == 0 {
            o.tx_waker = Some(cx.waker().clone());
            return Poll::Pending;
        }

        // we have enough permits -- poll for the next datagram
        let result = o.buf.poll_tx(cx).map(|x| x.map(ChannelDataFrame));

        match &result {
            Poll::Ready(Some(_)) => {
                o.our_available_permits -= 1;
            }
            Poll::Ready(None) => {
                *self = Self::Closed;
            }
            Poll::Pending => (),
        }

        result
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
            (Self::SendOpen { buf, mut waker }, Open) => {
                if let Some(w) = waker.take() {
                    w.wake();
                }
                (Self::Open(OpenChannel::new(buf)), None)
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

    pub(crate) fn closed(&self) -> bool {
        match self {
            ChannelState::Closed => true,
            _ => false,
        }
    }
}
