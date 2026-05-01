use super::{handle::ChannelHandle, shared::Shared};
use crate::frame::{ChannelAdmit, ChannelControlHeader, ChannelDataFrame};
use std::{
    fmt::Debug,
    sync::{Arc, Weak},
};

/// Representation of the channel's state.
pub struct Channel {
    status: Status,
}

impl Default for Channel {
    /// Default [Channel] for being constructed inside a map.
    fn default() -> Self {
        Self {
            status: Status::Closed(Closed {
                reason: CloseReason::NotOpened,
                send_reset: false,
            }),
        }
    }
}

enum Status {
    Closed(Closed),
    /// Sent an open, but the other side has not given us an open
    SentOpen(OpenState),
    /// The other side gave us an open, but we have yet to send an open
    RecvOpen,
    Open(OpenState),
}

impl Default for Status {
    /// Default [Status] for supporting [`std::mem::take`].
    fn default() -> Self {
        Status::Closed(Closed {
            reason: CloseReason::Panicked,
            send_reset: true,
        })
    }
}

/// State tracked after user requests an open.
pub struct OpenState {
    /// shared with user
    shared: Weak<Shared>,

    /// whether we still need to send an open or not
    need_to_send_open: bool,
}

impl OpenState {
    fn ensure_user_alive(&self) -> Option<Arc<Shared>> {
        self.shared.upgrade()
    }
}

/// State held by a closed channel.
#[derive(Debug)]
pub struct Closed {
    reason: CloseReason,
    send_reset: bool,
}

/// Why the channel is closed.
#[derive(Debug, thiserror::Error, Clone)]
pub enum CloseReason {
    #[error("Channel was never opened")]
    NotOpened,
    #[error("Panicked while processing something!")]
    Panicked,
    #[error("User dropped their handle to the channel")]
    UserDropped,
    #[error("Connection reset")]
    GotReset,
    #[error("Got an invalid frame! {state} + {frame}")]
    InvalidFrame { state: String, frame: String },
    #[error("Out of capacity! This may be the result of a logic error in stdiomux!")]
    OutOfCapacity,
}

impl Debug for Status {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Closed(r) => f.debug_tuple("Closed").field(r).finish(),
            Self::SentOpen(_) => write!(f, "SentOpen"),
            Self::RecvOpen => write!(f, "RecvOpen"),
            Self::Open(_) => write!(f, "Open"),
        }
    }
}

/// Make a pair of [UserHandle] and [ChannelHandle].
fn make_handles(our_rx_buffer: usize, need_to_send_open: bool) -> (OpenState, ChannelHandle) {
    let shared = Arc::new(Shared::new(our_rx_buffer));
    let for_us = OpenState {
        shared: Arc::downgrade(&shared),
        need_to_send_open,
    };
    let for_them = ChannelHandle { shared };
    (for_us, for_them)
}

#[derive(Debug, thiserror::Error)]
pub enum AcceptRxError {
    #[error("Buffer is out of capacity")]
    OutOfCapacity,
    #[error("Buffer is disconnected")]
    Disconnected,
}

#[derive(Debug, thiserror::Error)]
pub enum OpenChannelError {
    #[error("Channel is already in use")]
    ChannelInUse,

    #[error("Requestor dropped before channel could be opened")]
    RequestorDropped,

    #[error("Channel already closed: {0}")]
    ChannelClosed(#[from] CloseReason),
}

impl Channel {
    /// Called when user attempts to open the channel.
    pub fn request_open(
        &mut self,
        our_rx_buffer: usize,
    ) -> Result<ChannelHandle, OpenChannelError> {
        match &self.status {
            // don't open if already in use
            Status::SentOpen(_) | Status::Open(_) => Err(OpenChannelError::ChannelInUse),

            // combine our received open with the requested open
            Status::RecvOpen => {
                let (open_state, for_user) = make_handles(our_rx_buffer, true);
                self.status = Status::Open(open_state);
                Ok(for_user)
            }

            // channel has not yet been opened
            Status::Closed(Closed {
                reason: CloseReason::NotOpened,
                ..
            }) => {
                let (open_state, for_user) = make_handles(our_rx_buffer, true);
                self.status = Status::SentOpen(open_state);
                Ok(for_user)
            }

            // all other close reasons are an error
            Status::Closed(Closed {
                reason: other_reason,
                ..
            }) => Err(OpenChannelError::ChannelClosed(other_reason.clone())),
        }
    }

    /// Handle a data frame.
    pub fn on_recv_data(&mut self, data: ChannelDataFrame) {
        let Some(shared) = self.require_open_and_user() else {
            return;
        };

        match shared.rx.lock().unwrap().try_push(data.0) {
            Ok(_) => (),
            Err(_) => self.ensure_closed(CloseReason::OutOfCapacity, true),
        }
    }

    /// Handle a control frame.
    pub fn on_recv_control(&mut self, f: ChannelControlHeader) {
        match f {
            // if got reset, go into closed and don't send a reset back
            ChannelControlHeader::Reset => {
                self.ensure_closed(CloseReason::GotReset, false);
            }

            f @ ChannelControlHeader::Open => match std::mem::take(&mut self.status) {
                // transition into recvopen
                Status::Closed(_) => {
                    self.status = Status::RecvOpen;
                }

                // combine our sendopen and the existing open into an opened channel
                Status::SentOpen(h) => {
                    self.status = Status::Open(h);
                }

                // not allowed to get open in any other state
                s @ (Status::RecvOpen | Status::Open(_)) => {
                    self.ensure_closed(
                        CloseReason::InvalidFrame {
                            state: format!("{s:?}"),
                            frame: format!("{f:?}"),
                        },
                        true,
                    );
                }
            },

            ChannelControlHeader::Admit(adm) => {
                let Some(user) = self.require_open_and_user() else {
                    return;
                };
                user.tx.lock().unwrap().granted_permits += adm.permits();
            }
        }
    }

    /// Pull a control frame from this channel. Updates the state of the channel.
    pub fn next_control_send(&mut self) -> Option<ChannelControlHeader> {
        match &mut self.status {
            Status::Closed(closed) if closed.send_reset => {
                closed.send_reset = false;
                Some(ChannelControlHeader::Reset)
            }
            Status::SentOpen(open_state) if open_state.need_to_send_open => {
                open_state.need_to_send_open = false;
                Some(ChannelControlHeader::Open)
            }
            Status::Open(_) => {
                let shared = self.require_open_and_user()?;
                let mut rx = shared.rx.lock().unwrap();
                let desired_permits_to_grant = rx
                    .available_capacity()
                    .checked_sub(rx.granted_permits)
                    .expect(
                        "Capacity is less than granted permits! This is a logic error in stdiomux!",
                    );
                let adm = ChannelAdmit::grant_up_to(desired_permits_to_grant)?;

                rx.granted_permits -= adm.permits();
                Some(ChannelControlHeader::Admit(adm))
            }
            _ => None,
        }
    }

    /// Pull a data frame from this channel. Consumes a permit if there's data available.
    pub fn next_data_send(&mut self) -> Option<ChannelDataFrame> {
        let Some(shared) = self.ensure_open_and_user() else {
            return None;
        };

        let mut tx = shared.tx.lock().unwrap();

        // ensure we can send
        let new_permits_after_send = tx.granted_permits.checked_sub(1)?;
        let out = tx.try_pop().ok()?;

        // now we can decrement counter because it seems we clearly can
        tx.granted_permits = new_permits_after_send;

        Some(out.into())
    }

    /// Returns if this channel is closed or not
    pub(crate) fn closed(&self) -> bool {
        match self.status {
            Status::Closed(_) => true,
            _ => false,
        }
    }

    /// Helper function that checks if we're open and the user is listening.
    ///
    /// Unlike [Self::require_open_and_user], this will not close us if this is not the case.
    fn ensure_open_and_user(&mut self) -> Option<Arc<Shared>> {
        let Status::Open(h) = &self.status else {
            return None;
        };
        h.ensure_user_alive()
    }

    /// Helper function that checks if we're open and the user is listening.
    ///
    /// NOTE: This will move us into closed if we're not open! Use [Self::ensure_open_and_user]
    /// if this is not desired.
    fn require_open_and_user(&mut self) -> Option<Arc<Shared>> {
        let Status::Open(h) = &self.status else {
            self.ensure_closed(CloseReason::UserDropped, true);
            return None;
        };
        h.ensure_user_alive()
    }

    /// Close the channel with the given reason. Does nothing if already closed.
    pub fn ensure_closed(&mut self, reason: CloseReason, send_reset: bool) {
        if self.closed() {
            return;
        }
        self.status = Status::Closed(Closed { reason, send_reset });
    }
}
