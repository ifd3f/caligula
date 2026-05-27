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
use serde::{Deserialize, Serialize, de::DeserializeOwned};

use crate::{herder_api::write_verify::WVAction, util::layer_error::LayerError};

#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Serialize,
    Deserialize,
    strum::EnumDiscriminants,
    derive_more::TryInto,
    derive_more::From,
)]
#[strum_discriminants(name(RequestTag), derive(Serialize, Deserialize))]
pub enum Request {
    WriteVerify(WVAction),
}

/// A generic service for starting and managing individual [`HerderAction`]s.
#[auto_impl(&, Box, Rc, Arc)]
pub trait HerderActionService<A: HerderAction> {
    /// Errors that the transport of this [`HerderActionService`] may introduce.
    type Error: Error;

    /// Start the given [`HerderAction`].
    async fn start(
        &self,
        action: A,
    ) -> Result<HerderActionResponse<A, Self::Error>, LayerError<A::Error, Self::Error>>;
}

/// Successful response from a [`HerderActionService`].
pub struct HerderActionResponse<A: HerderAction, E> {
    /// Initialization information.
    pub start: A::Start,

    /// Stream of events that may come out of this.
    #[expect(clippy::type_complexity)]
    pub events: LocalBoxStream<'static, Result<A::Event, LayerError<A::Error, E>>>,
}

/// Arbitrary long-running action that emits a stream of events. This can be
/// anything, from writing to verifying to voiding.
pub trait HerderAction: Message {
    /// Successful initialization data.
    type Start: Message;

    /// Errors that may be encountered.
    type Error: Message + Error;

    /// The events emitted by the herd after start.
    type Event: Message;
}

/// Trait alias for things we can work with on the wire or in RPC.
pub trait Message:
    Serialize + DeserializeOwned + Debug + Clone + PartialEq + Send + 'static
{
}

impl<T: Serialize + DeserializeOwned + Debug + Clone + PartialEq + Send + 'static> Message for T {}
