use std::fmt::{Debug, Display};

use serde::{Deserialize, Serialize, de::DeserializeOwned};

pub use super::writer_process::ipc::{WriteVerifyAction, WriteVerifyError, WriteVerifyEvent};

/// Tell the herder to start a herd for performing an arbitrary action.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StartHerd<A> {
    /// ID to associate with all of the herd's events
    pub id: u64,

    /// The action to perform
    pub action: A,
}

/// Arbitrary herd initialization action. This can be anything, from writing to verifying to voiding.
pub trait HerdAction: IpcObject {
    /// The initial information variant that it's expected to send out as soon as it
    /// has started running.
    type StartInfo: IpcObject;

    /// A failure variant indicating that this herd has terminated unexpectedly and fatally
    /// without any hope of recovery.
    type Failure: IpcObject + Display;

    /// The events emitted by the herd afterwards.
    type Event: IpcObject;
}

/// An event emitted by a running herd.
pub trait IpcObject:
    Serialize
    + DeserializeOwned
    + Debug
    + Clone
    + PartialEq
    + Send
    + 'static
{
}

impl <T> IpcObject for T where T:
    Serialize
    + DeserializeOwned
    + Debug
    + Clone
    + PartialEq
    + Send
    + 'static{}

/// An enum containing all implemented and valid types of herder event.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum HerdMessage<I, F, E> {
    Initial(I),
    Failure(F),
    Event(E),
    Done,
}
