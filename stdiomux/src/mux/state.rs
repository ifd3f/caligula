use std::collections::HashMap;

use crate::frame::{ChannelControlHeader, ChannelDataFrame, ChannelId, Frame, MuxControlHeader};

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum MuxState {
    Active(ActiveData),
    Terminating(ActiveData),
    Closed(Result<(), ClosedReason>),
}

impl Default for MuxState {
    fn default() -> Self {
        Self::Closed(Err(ClosedReason::Panicked))
    }
}

#[derive(Debug, PartialEq, Eq, thiserror::Error, Clone)]
pub enum ClosedReason {
    #[error("Connection reset")]
    Reset,
    #[error("Got frame {0:?}")]
    UnexpectedFrame(Frame),
    #[error("Panicked during operation")]
    Panicked,
}

#[derive(Debug, Default, PartialEq, Eq, Clone)]
pub struct ActiveData {
    /// Active, non-closed channels.
    channels: HashMap<ChannelId, ChannelMapEntry>,
}

impl ActiveData {
    fn on_channel_data(&mut self, id: ChannelId, f: ChannelDataFrame) -> Option<Frame> {
        todo!()
    }

    fn on_channel_control(&mut self, id: ChannelId, f: ChannelControlHeader) -> Option<Frame> {
        todo!()
    }
}

impl MuxState {
    /// Create an initial post-handshake [MuxState]
    pub fn opened() -> Self {
        Self::Active(ActiveData::default())
    }

    /// Handle receiving the given frame.
    ///
    /// Returns the new state, along with the reply to send, if any.
    pub fn on_recv(self, frame: Frame) -> (Self, Option<Frame>) {
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
}

#[derive(Debug, PartialEq, Eq, Clone)]
struct ChannelMapEntry {
    // /// current state of the connection
    //state: std::sync::Mutex<ChannelState>,
}

impl ChannelMapEntry {
    fn on_recv_data(&self, _data: ChannelDataFrame) {
        todo!()
    }

    fn on_recv_adm(&self, _permits: u8) {
        todo!()
    }
}

#[cfg(test)]
mod tests {
    use bytes::Bytes;
    use rstest::rstest;

    use super::*;
    use crate::frame::{
        ChannelControlHeader, ChannelDataFrame, ChannelId, Frame, MuxControlHeader,
    };

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
    fn test_hello_frame_handling(
        #[case] initial_state: MuxState,
        #[case] expected_state: MuxState,
    ) {
        let frame = Frame::MuxControl(MuxControlHeader::Hello);
        let (new_state, reply) = initial_state.on_recv(frame);

        assert_eq!(new_state, expected_state);
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
        #[case] initial_state: MuxState,
        #[case] expected_state: MuxState,
        #[case] expected_reply: Option<Frame>,
    ) {
        let frame = Frame::MuxControl(MuxControlHeader::Reset);
        let (new_state, reply) = initial_state.on_recv(frame);

        assert_eq!(new_state, expected_state);
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
        #[case] initial_state: MuxState,
        #[case] expected_state: MuxState,
        #[case] expected_reply: Option<Frame>,
    ) {
        let frame = Frame::MuxControl(MuxControlHeader::Finished);
        let (new_state, reply) = initial_state.on_recv(frame);

        assert_eq!(new_state, expected_state);
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
        #[case] initial_state: MuxState,
        #[case] expected_state: MuxState,
        #[case] expected_reply: Option<Frame>,
    ) {
        let frame = Frame::MuxControl(MuxControlHeader::Terminate);
        let (new_state, reply) = initial_state.on_recv(frame);

        assert_eq!(new_state, expected_state);
        assert_eq!(reply, expected_reply);
    }

    #[rstest]
    #[case::terminate(Frame::MuxControl(MuxControlHeader::Terminate))]
    #[case::finished(Frame::MuxControl(MuxControlHeader::Finished))]
    fn test_non_reset_frame_when_closed_sends_reset(#[case] frame: Frame) {
        let state = MuxState::Closed(Ok(()));
        let (new_state, reply) = state.on_recv(frame);

        assert_eq!(new_state, MuxState::Closed(Ok(())));
        assert_eq!(reply, Some(Frame::MuxControl(MuxControlHeader::Reset)));
    }

    #[rstest]
    #[case::active(MuxState::opened())]
    #[case::terminating(MuxState::Terminating(ActiveData::default()))]
    fn test_channel_data_preserves_state(#[case] initial_state: MuxState) {
        let frame = Frame::ChannelData(ChannelId(1), ChannelDataFrame(Bytes::new()));
        let expected_state = initial_state.clone();

        let (new_state, _reply) = initial_state.on_recv(frame);

        assert_eq!(new_state, expected_state);
    }

    #[rstest]
    #[case::active(MuxState::opened())]
    #[case::terminating(MuxState::Terminating(ActiveData::default()))]
    fn test_channel_control_preserves_state(#[case] initial_state: MuxState) {
        let frame = Frame::ChannelControl(ChannelId(1), ChannelControlHeader::Open);
        let expected_state = initial_state.clone();

        let (new_state, _reply) = initial_state.on_recv(frame);

        assert_eq!(new_state, expected_state);
    }

    #[test]
    fn test_graceful_shutdown_sequence() {
        // Start with Active state
        let state = MuxState::opened();

        // Receive Terminate -> should transition to Terminating
        let (state, reply) = state.on_recv(Frame::MuxControl(MuxControlHeader::Terminate));
        assert_eq!(state, MuxState::Terminating(ActiveData::default()));
        assert_eq!(reply, Some(Frame::MuxControl(MuxControlHeader::Terminate)));

        // Receive Finished -> should close gracefully
        let (state, reply) = state.on_recv(Frame::MuxControl(MuxControlHeader::Finished));
        assert_eq!(state, MuxState::Closed(Ok(())));
        assert_eq!(reply, None);
    }

    #[test]
    fn test_unexpected_frame_after_reset() {
        // Close with Reset
        let state = MuxState::opened();
        let (state, _) = state.on_recv(Frame::MuxControl(MuxControlHeader::Reset));
        assert_eq!(state, MuxState::Closed(Err(ClosedReason::Reset)));

        // Try to send Terminate - should get Reset back
        let (state, reply) = state.on_recv(Frame::MuxControl(MuxControlHeader::Terminate));
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
        let state = MuxState::Closed(Ok(()));

        // First reset
        let (state, reply) = state.on_recv(Frame::MuxControl(MuxControlHeader::Reset));
        assert_eq!(state, MuxState::Closed(Ok(())));
        assert_eq!(reply, None); // No response to prevent cycles

        // Second reset
        let (state, reply) = state.on_recv(Frame::MuxControl(MuxControlHeader::Reset));
        assert_eq!(state, MuxState::Closed(Ok(())));
        assert_eq!(reply, None); // Still no response
    }

    #[test]
    fn test_state_preserves_closure_reason() {
        // Close with Reset
        let state = MuxState::Closed(Err(ClosedReason::Reset));

        // Send another frame
        let (new_state, _) = state.on_recv(Frame::MuxControl(MuxControlHeader::Terminate));

        // Should still be closed with original reason
        assert_eq!(new_state, MuxState::Closed(Err(ClosedReason::Reset)));
    }
}
