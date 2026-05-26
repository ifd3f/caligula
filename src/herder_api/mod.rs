//! Defines traits and modules for IPC between the child process and parent
//! process.
//!
//! The UI may speak some of these types, but it should prefer to use the
//! higher-level interfaces defined in [`crate::facade`].

pub mod client;
pub mod error;
pub mod server;
pub mod write_verify;

use std::{error::Error, fmt::Debug};

use auto_impl::auto_impl;
use futures::stream::LocalBoxStream;
use serde::{Serialize, de::DeserializeOwned};

use crate::herder_api::error::LayerError;

pub struct HerderResponse<A: HerderAction, E> {
    pub start: A::Start,

    /// Outer [`Result`] represents transport failures, inner [`Result`]
    /// represents application-level failures.
    #[expect(clippy::type_complexity)]
    pub events: LocalBoxStream<'static, Result<A::Event, LayerError<A::Error, E>>>,
}

#[auto_impl(&, Box, Rc, Arc)]
pub trait HerderService<A: HerderAction> {
    /// Errors that this transport may introduce.
    type Error: Error;

    /// Outer [`Result`] represents transport failures, inner [`Result`]
    /// represents application-level failures.
    async fn start(
        &self,
        action: A,
    ) -> Result<HerderResponse<A, Self::Error>, LayerError<A::Error, Self::Error>>;
}

/// Arbitrary herd initialization action. This can be anything, from writing to
/// verifying to voiding.
pub trait HerderAction: Message {
    type Start: Message;

    type Error: Message + Error;

    /// The events emitted by the herd afterwards.
    type Event: Message;
}

/// Trait alias for things we can work with on the wire or in RPC.
pub trait Message:
    Serialize + DeserializeOwned + Debug + Clone + PartialEq + Send + 'static
{
}

impl<T: Serialize + DeserializeOwned + Debug + Clone + PartialEq + Send + 'static> Message for T {}
