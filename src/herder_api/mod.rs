pub mod write_verify;

use std::fmt::{Debug, Display};

use serde::{Deserialize, Serialize, de::DeserializeOwned};

/// Tell the herder to start a herd for performing an arbitrary action.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StartHerd<A> {
    /// ID to associate with all of the herd's events
    pub id: u64,

    /// The action to perform
    pub action: A,
}

/// Arbitrary herd initialization action. This can be anything, from writing to verifying to voiding.
pub trait HerdAction:
    Serialize + DeserializeOwned + Debug + Clone + PartialEq + Send + 'static
{
    /// The events emitted by the herd afterwards.
    type Event: HerdEvent;
}

/// An event emitted by a running herd.
pub trait HerdEvent:
    Serialize
    + DeserializeOwned
    + Debug
    + Clone
    + PartialEq
    + TryFrom<TopLevelHerdEvent, Error = TopLevelHerdEvent>
    + Send
    + 'static
{
    /// The initial information variant that it's expected to send out as soon as it
    /// has started running.
    type StartInfo: Debug;

    /// A failure variant indicating that this herd has terminated unexpectedly and fatally
    /// without any hope of recovery.
    type Failure: Display + Debug;

    /// Downcast this event trait into its InitialInfo variant.
    fn downcast_as_initial_info(self) -> Result<Self::StartInfo, Self>;

    /// Downcast this event trait into its failure variant.
    fn downcast_as_failure(self) -> Result<Self::Failure, Self>;
}

/// An enum containing all implemented and valid types of herder event.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, derive_more::From)]
#[non_exhaustive]
pub enum TopLevelHerdEvent {
    Writer(write_verify::WriteVerifyEvent),
}

macro_rules! impl_try_from_top_level_herd_event {
    ($arm:ident => $event_type:ty) => {
        impl TryFrom<crate::herder_api::TopLevelHerdEvent> for $event_type {
            type Error = crate::herder_api::TopLevelHerdEvent;
            fn try_from(
                ev: crate::herder_api::TopLevelHerdEvent,
            ) -> Result<Self, crate::herder_api::TopLevelHerdEvent> {
                match ev {
                    crate::herder_api::TopLevelHerdEvent::$arm(x) => Ok(x),
                    //other => Err(other),
                }
            }
        }
    };
}

pub(self) use impl_try_from_top_level_herd_event;
