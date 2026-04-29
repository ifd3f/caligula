use std::collections::HashMap;

use crate::frame::{ChannelControlHeader, ChannelDataFrame, ChannelId, Frame, MuxControlHeader};

#[derive(Debug, Clone)]
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

#[derive(Debug, thiserror::Error, Clone)]
pub enum ClosedReason {
    #[error("Connection reset")]
    Reset,
    #[error("Got frame {0:?}")]
    UnexpectedFrame(Frame),
    #[error("Panicked during operation")]
    Panicked,
}

#[derive(Debug, Default, Clone)]
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
            (Self::Active(mut a) | Self::Terminating(mut a), ChannelData(id, f)) => {
                let response = a.on_channel_data(id, f);
                (Self::Active(a), response)
            }

            // Handle channel control
            (Self::Active(mut a) | Self::Terminating(mut a), ChannelControl(id, f)) => {
                let response = a.on_channel_control(id, f);
                (Self::Active(a), response)
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

#[derive(Debug, Clone)]
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
