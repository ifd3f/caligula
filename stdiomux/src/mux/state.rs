use std::error::Error;
use std::fmt::Debug;
use std::sync::Arc;
use std::task::Context;
use std::{collections::HashMap, task::Poll};

use crate::channel::state::{ChannelBufferFactory, OpenChannelError};
use crate::{
    channel::state::{ChannelBuffer, ChannelState},
    frame::{ChannelControlHeader, ChannelDataFrame, ChannelId, Frame, MuxControlHeader},
};

pub enum MuxState<B: ChannelBuffer> {
    Active(ActiveData<B>),
    Terminating(ActiveData<B>),
    Closed(Result<(), ClosedReason>),
}

impl<B: ChannelBuffer> Clone for MuxState<B> {
    fn clone(&self) -> Self {
        match self {
            Self::Active(arg0) => Self::Active(arg0.clone()),
            Self::Terminating(arg0) => Self::Terminating(arg0.clone()),
            Self::Closed(arg0) => Self::Closed(arg0.clone()),
        }
    }
}

impl<B: ChannelBuffer> PartialEq for MuxState<B> {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Active(l0), Self::Active(r0)) => l0 == r0,
            (Self::Terminating(l0), Self::Terminating(r0)) => l0 == r0,
            (Self::Closed(l0), Self::Closed(r0)) => l0 == r0,
            _ => false,
        }
    }
}

impl<B: ChannelBuffer> Debug for MuxState<B> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Active(arg0) => f.debug_tuple("Active").field(arg0).finish(),
            Self::Terminating(arg0) => f.debug_tuple("Terminating").field(arg0).finish(),
            Self::Closed(arg0) => f.debug_tuple("Closed").field(arg0).finish(),
        }
    }
}

impl<B: ChannelBuffer> Default for MuxState<B> {
    fn default() -> Self {
        Self::Closed(Err(ClosedReason::Panicked))
    }
}

#[derive(Debug, thiserror::Error, Clone)]
pub enum ClosedReason {
    #[error("Connection reset")]
    Reset,
    #[error("Got frame {0:?}")]
    UnexpectedFrame(Frame),
    #[error("Panicked during operation")]
    Panicked,
    #[error("Transport failure: {0}")]
    TransportFailure(#[from] Arc<dyn Error + Sync + Send>),
    #[error("Transport unexpectedly closed")]
    TransportClosed,
    #[error("Queue unexpectedly closed")]
    QueueClosed,
}

impl PartialEq for ClosedReason {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::UnexpectedFrame(l0), Self::UnexpectedFrame(r0)) => l0 == r0,
            (Self::TransportFailure(_), Self::TransportFailure(_)) => false,
            _ => core::mem::discriminant(self) == core::mem::discriminant(other),
        }
    }
}

pub struct ActiveData<B: ChannelBuffer> {
    /// Active, non-closed channels.
    channels: HashMap<ChannelId, ChannelMapEntry<B>>,
}

impl<B: ChannelBuffer> Debug for ActiveData<B> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ActiveData")
            .field("channels", &self.channels)
            .finish()
    }
}

impl<B: ChannelBuffer> Default for ActiveData<B> {
    fn default() -> Self {
        Self {
            channels: Default::default(),
        }
    }
}

impl<B: ChannelBuffer> Clone for ActiveData<B> {
    fn clone(&self) -> Self {
        Self {
            channels: self
                .channels
                .keys()
                .map(|k| (*k, ChannelMapEntry::default()))
                .collect(),
        }
    }
}

impl<B: ChannelBuffer> ActiveData<B> {
    fn on_channel_data(&mut self, id: ChannelId, f: ChannelDataFrame) -> Option<Frame> {
        self.channels
            .entry(id)
            .or_default()
            .state
            .on_recv_data(f)
            .map(|f| Frame::ChannelControl(id, f))
    }

    fn on_channel_control(&mut self, id: ChannelId, f: ChannelControlHeader) -> Option<Frame> {
        let cell = self.channels.entry(id).or_default();
        let (new, out) = std::mem::take(cell).state.on_recv_control(f);
        cell.state = new;

        out.map(|f| Frame::ChannelControl(id, f))
    }

    /// Get frames to send. This function is guaranteed to return a fixed number of frames
    /// per channel, but it may return more than one frame per channel.
    #[inline]
    pub fn poll_sends(&mut self, cx: &mut Context<'_>) -> Vec<Frame> {
        let mut out = vec![];
        self.clean_up_closed_channels();
        for (id, c) in &mut self.channels {
            if let Poll::Ready(x) = c.state.poll_next_adm(cx) {
                out.push(Frame::ChannelControl(*id, x));
            }
            if let Poll::Ready(Some(x)) = c.state.poll_next_tx(cx) {
                out.push(Frame::ChannelData(*id, x));
            }
        }
        self.clean_up_closed_channels();

        out
    }

    pub fn clean_up_closed_channels(&mut self) {
        self.channels.retain(|_, v| !v.state.closed());
    }
}

impl<B: ChannelBuffer> PartialEq for ActiveData<B> {
    fn eq(&self, other: &Self) -> bool {
        self.channels.keys().eq(other.channels.keys())
    }
}

#[derive(Debug, thiserror::Error)]
#[error("Mux is closed")]
pub struct MuxNotOpen;

impl<B: ChannelBuffer> MuxState<B> {
    /// Create an initial post-handshake [MuxState]
    pub fn opened() -> Self {
        Self::Active(ActiveData::default())
    }

    /// Poll for a single round of frames to send. Returns a list of frames, or an error if the mux
    /// is in the closed state.
    ///
    /// This function is guaranteed to return a fixed number of frames per channel, but it may return
    /// more than one frame per channel.
    pub fn poll_sends(&mut self, cx: &mut Context<'_>) -> Result<Vec<Frame>, MuxNotOpen> {
        let (Self::Active(a) | Self::Terminating(a)) = self else {
            return Err(MuxNotOpen);
        };
        Ok(a.poll_sends(cx))
    }

    /// Handle receiving the given frame.
    ///
    /// Returns the new state, along with the reply to send, if any.
    pub fn on_recv(&mut self, f: Frame) -> Option<Frame> {
        let (new_state, f) = std::mem::take(self).on_recv_owned(f);
        *self = new_state;
        f
    }

    #[inline]
    fn on_recv_owned(self, frame: Frame) -> (Self, Option<Frame>) {
        use Frame::*;
        use MuxControlHeader::*;

        match (self, frame) {
            // If we got a hello but we're already closed, send a reset
            (s @ Self::Closed(_), MuxControl(Hello)) => (s, Some(Frame::MuxControl(Reset))),

            // If got a hello in any other state, transition into closed and send a reset
            (_, f @ MuxControl(Hello))
            | (Self::Active(_), f @ MuxControl(Finished))
            | (Self::Terminating(_), f @ MuxControl(Terminate)) => (
                MuxState::Closed(Err(ClosedReason::UnexpectedFrame(f))),
                Some(Frame::MuxControl(Reset)),
            ),

            // If got reset and already closed, no-op to prevent reset cycles
            (s @ Self::Closed(_), MuxControl(Reset)) => (s, None),

            // Any reset in any other state is a shutdown
            (_, MuxControl(Reset)) => (Self::Closed(Err(ClosedReason::Reset)), None),

            // If closed and got anything besides a reset, send a reset
            (s @ Self::Closed(_), _) => (s, Some(Frame::MuxControl(Reset))),

            // Handle channel data
            (Self::Active(mut a), ChannelData(id, f)) => {
                let response = a.on_channel_data(id, f);
                (Self::Active(a), response)
            }
            (Self::Terminating(mut a), ChannelData(id, f)) => {
                let response = a.on_channel_data(id, f);
                (Self::Terminating(a), response)
            }

            // Handle channel control
            (Self::Active(mut a), ChannelControl(id, f)) => {
                let response = a.on_channel_control(id, f);
                (Self::Active(a), response)
            }
            // Reject new connections if terminating
            (Self::Terminating(mut a), ChannelControl(id, ChannelControlHeader::Open)) => {
                a.channels.remove(&id);
                (
                    Self::Terminating(a),
                    Some(Frame::ChannelControl(id, ChannelControlHeader::Reset)),
                )
            }
            // Otherwise, handle control messages normally
            (Self::Terminating(mut a), ChannelControl(id, f)) => {
                let response = a.on_channel_control(id, f);
                (Self::Terminating(a), response)
            }

            // Active transitioning into terminating
            (Self::Active(a), MuxControl(Terminate)) => {
                (Self::Terminating(a), Some(Frame::MuxControl(Terminate)))
            }

            // Handle successful, graceful termination
            (Self::Terminating(_), MuxControl(Finished)) => (Self::Closed(Ok(())), None),
        }
    }

    pub(crate) fn closed(&self) -> bool {
        match self {
            MuxState::Closed(_) => true,
            _ => false,
        }
    }

    pub(crate) fn open_channel(
        &mut self,
        channel_id: ChannelId,
        f: Box<dyn ChannelBufferFactory<Output = B>>,
    ) -> Poll<Result<(), OpenChannelError>> {
        let Self::Active(a) = self else {
            // if terminating, return this error too
            return Poll::Ready(Err(MuxNotOpen.into()));
        };
        a.channels
            .entry(channel_id)
            .or_default()
            .state
            .request_open(f)
    }
}

struct ChannelMapEntry<B: ChannelBuffer> {
    /// current state of the connection
    state: ChannelState<B>,
}

impl<B: ChannelBuffer> Debug for ChannelMapEntry<B> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ChannelMapEntry")
            .field("state", &self.state)
            .finish()
    }
}

impl<B: ChannelBuffer> Default for ChannelMapEntry<B> {
    fn default() -> Self {
        Self {
            state: Default::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;
    use crate::{
        channel::state::NullChannelBuffer,
        frame::{Frame, MuxControlHeader},
    };

    type MuxState = super::MuxState<NullChannelBuffer>;
    type ActiveData = super::ActiveData<NullChannelBuffer>;

    #[test]
    fn test_muxstate_default_is_closed_panicked() {
        let state = MuxState::default();
        assert_eq!(state, MuxState::Closed(Err(ClosedReason::Panicked)));
    }

    #[test]
    fn test_muxstate_opened_creates_active_state() {
        let state = MuxState::opened();
        assert_eq!(state, MuxState::Active(ActiveData::default()));
    }

    #[rstest]
    #[case::closed_ok(MuxState::Closed(Ok(())), MuxState::Closed(Ok(())))]
    #[case::closed_reset(
        MuxState::Closed(Err(ClosedReason::Reset)),
        MuxState::Closed(Err(ClosedReason::Reset))
    )]
    #[case::active(
        MuxState::opened(),
        MuxState::Closed(Err(ClosedReason::UnexpectedFrame(Frame::MuxControl(
            MuxControlHeader::Hello
        ))))
    )]
    #[case::terminating(
        MuxState::Terminating(ActiveData::default()),
        MuxState::Closed(Err(ClosedReason::UnexpectedFrame(Frame::MuxControl(
            MuxControlHeader::Hello
        ))))
    )]
    fn test_hello_frame_handling(#[case] mut state: MuxState, #[case] expected_state: MuxState) {
        let frame = Frame::MuxControl(MuxControlHeader::Hello);
        let reply = state.on_recv(frame);

        assert_eq!(state, expected_state);
        assert_eq!(reply, Some(Frame::MuxControl(MuxControlHeader::Reset)));
    }

    #[rstest]
    #[case::closed_ok(MuxState::Closed(Ok(())), MuxState::Closed(Ok(())), None)]
    #[case::closed_reset(
        MuxState::Closed(Err(ClosedReason::Reset)),
        MuxState::Closed(Err(ClosedReason::Reset)),
        None
    )]
    #[case::active(MuxState::opened(), MuxState::Closed(Err(ClosedReason::Reset)), None)]
    #[case::terminating(
        MuxState::Terminating(ActiveData::default()),
        MuxState::Closed(Err(ClosedReason::Reset)),
        None
    )]
    fn test_reset_frame_handling(
        #[case] mut state: MuxState,
        #[case] expected_state: MuxState,
        #[case] expected_reply: Option<Frame>,
    ) {
        let frame = Frame::MuxControl(MuxControlHeader::Reset);
        let reply = state.on_recv(frame);

        assert_eq!(state, expected_state);
        assert_eq!(reply, expected_reply);
    }

    #[rstest]
    #[case::active(
        MuxState::opened(),
        MuxState::Closed(Err(ClosedReason::UnexpectedFrame(Frame::MuxControl(
            MuxControlHeader::Finished
        )))),
        Some(Frame::MuxControl(MuxControlHeader::Reset))
    )]
    #[case::terminating(
        MuxState::Terminating(ActiveData::default()),
        MuxState::Closed(Ok(())),
        None
    )]
    fn test_finished_frame_handling(
        #[case] mut state: MuxState,
        #[case] expected_state: MuxState,
        #[case] expected_reply: Option<Frame>,
    ) {
        let frame = Frame::MuxControl(MuxControlHeader::Finished);
        let reply = state.on_recv(frame);

        assert_eq!(state, expected_state);
        assert_eq!(reply, expected_reply);
    }

    #[rstest]
    #[case::active(
        MuxState::opened(),
        MuxState::Terminating(ActiveData::default()),
        Some(Frame::MuxControl(MuxControlHeader::Terminate))
    )]
    #[case::terminating(
        MuxState::Terminating(ActiveData::default()),
        MuxState::Closed(Err(ClosedReason::UnexpectedFrame(Frame::MuxControl(
            MuxControlHeader::Terminate
        )))),
        Some(Frame::MuxControl(MuxControlHeader::Reset))
    )]
    fn test_terminate_frame_handling(
        #[case] mut state: MuxState,
        #[case] expected_state: MuxState,
        #[case] expected_reply: Option<Frame>,
    ) {
        let frame = Frame::MuxControl(MuxControlHeader::Terminate);
        let reply = state.on_recv(frame);

        assert_eq!(state, expected_state);
        assert_eq!(reply, expected_reply);
    }

    #[rstest]
    #[case::terminate(Frame::MuxControl(MuxControlHeader::Terminate))]
    #[case::finished(Frame::MuxControl(MuxControlHeader::Finished))]
    fn test_non_reset_frame_when_closed_sends_reset(#[case] frame: Frame) {
        let mut state = MuxState::Closed(Ok(()));
        let reply = state.on_recv(frame);

        assert_eq!(state, MuxState::Closed(Ok(())));
        assert_eq!(reply, Some(Frame::MuxControl(MuxControlHeader::Reset)));
    }

    #[test]
    fn test_graceful_shutdown_sequence() {
        // Start with Active state
        let mut state = MuxState::opened();

        // Receive Terminate -> should transition to Terminating
        let reply = state.on_recv(Frame::MuxControl(MuxControlHeader::Terminate));
        assert_eq!(state, MuxState::Terminating(ActiveData::default()));
        assert_eq!(reply, Some(Frame::MuxControl(MuxControlHeader::Terminate)));

        // Receive Finished -> should close gracefully
        let reply = state.on_recv(Frame::MuxControl(MuxControlHeader::Finished));
        assert_eq!(state, MuxState::Closed(Ok(())));
        assert_eq!(reply, None);
    }

    #[test]
    fn test_unexpected_frame_after_reset() {
        // Close with Reset
        let mut state = MuxState::opened();
        let _ = state.on_recv(Frame::MuxControl(MuxControlHeader::Reset));
        assert_eq!(state, MuxState::Closed(Err(ClosedReason::Reset)));

        // Try to send Terminate - should get Reset back
        let reply = state.on_recv(Frame::MuxControl(MuxControlHeader::Terminate));
        assert_eq!(state, MuxState::Closed(Err(ClosedReason::Reset)));
        assert_eq!(reply, Some(Frame::MuxControl(MuxControlHeader::Reset)));
    }

    #[test]
    fn test_closed_reason_display() {
        let reason = ClosedReason::Reset;
        assert_eq!(reason.to_string(), "Connection reset");

        let reason = ClosedReason::Panicked;
        assert_eq!(reason.to_string(), "Panicked during operation");
    }

    #[test]
    fn test_closed_reason_clone() {
        let reason1 = ClosedReason::Reset;
        let reason2 = reason1.clone();
        assert_eq!(reason1, reason2);
    }

    #[test]
    fn test_active_data_default() {
        let data = ActiveData::default();
        assert_eq!(data.channels.len(), 0);
    }

    #[test]
    fn test_multiple_resets_dont_cycle() {
        let mut state = MuxState::Closed(Ok(()));

        // First reset
        let reply = state.on_recv(Frame::MuxControl(MuxControlHeader::Reset));
        assert_eq!(state, MuxState::Closed(Ok(())));
        assert_eq!(reply, None); // No response to prevent cycles

        // Second reset
        let reply = state.on_recv(Frame::MuxControl(MuxControlHeader::Reset));
        assert_eq!(state, MuxState::Closed(Ok(())));
        assert_eq!(reply, None); // Still no response
    }

    #[test]
    fn test_state_preserves_closure_reason() {
        // Close with Reset
        let mut state = MuxState::Closed(Err(ClosedReason::Reset));

        // Send another Reset frame
        state.on_recv(Frame::MuxControl(MuxControlHeader::Terminate));

        // Should still be closed with original reason
        assert_eq!(state, MuxState::Closed(Err(ClosedReason::Reset)));
    }
}
