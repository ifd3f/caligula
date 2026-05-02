use std::{
    error::Error,
    fmt::Debug,
    hash::Hash,
    task::{Context, Poll},
};

use auto_impl::auto_impl;
use bytes::Bytes;

/// Generic trait for an object controlling the multiplexing of data over a single channel.
///
/// [`MuxController`]s must run all necessary tasks in the background until they are dropped,
/// or an early termination condition is reached.
#[auto_impl(&, Box, Arc)]
pub trait MuxController: Send + Sync + 'static {
    /// Handle for channels being muxed by this mux controller.
    type ChannelHandle: ChannelHandle;

    /// A unique identifier for requesting channels in this [`MuxController`].
    ///
    /// Channel IDs are allocated by the caller, not the MuxController. It's up to the
    /// specific implementation to decide if opening a channel twice with the same ID
    /// is valid or not.
    type ChannelId: Debug + PartialEq + Eq + Clone + Ord + Hash;

    /// Why this [`MuxController`] was closed early.
    type ClosedReason: Error;

    /// Errors encountered while opening a channel.
    type OpenChannelError: Error;

    /// Assert that this mux controller is open. Returns an error if it's not.
    fn assert_open(&self) -> Result<(), Self::ClosedReason>;

    /// Attempt to open a new channel.
    ///
    /// Returns the handle to the channel, or an error if the channel could not be opened.
    ///
    /// This returns immediately, but the channel may not be fully opened when this returns.
    fn open_channel(
        &self,
        id: &Self::ChannelId,
    ) -> Result<Self::ChannelHandle, Self::OpenChannelError>;
}

/// Handle to a single channel inside a [`MuxController`].
///
/// When dropped, the channel is closed.
#[auto_impl(&, Box, Arc)]
pub trait ChannelHandle: Send + Sync + Sized + 'static {
    /// Largest item allowed to be sent or received. Panics may occur if an item larger
    /// than this is placed in.
    const MAX: usize;

    /// Reason this channel is closed.
    type ClosedReason: Error;

    /// Assert that this channel is open, or returns an error if it's not.
    fn assert_open(&self) -> Result<(), Self::ClosedReason>;

    /// Attempt to queue the provided item for sending.
    ///
    /// Items will only be queued if [`Poll::Ready`] is returned.
    fn poll_send(&self, cx: &mut Context<'_>, bs: &Bytes) -> Poll<Result<(), Self::ClosedReason>>;

    /// Attempt to pull an item from the request queue.
    ///
    /// Items will only be removed from the queue if [`Poll::Ready`] is returned.
    fn poll_recv(&self, cx: &mut Context<'_>) -> Poll<Result<Bytes, Self::ClosedReason>>;
}
