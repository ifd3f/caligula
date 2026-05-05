pub mod write_verify;
use bytes::{Bytes, BytesMut};
use futures::{
    Stream, StreamExt,
    stream::{self, BoxStream},
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::{
    error::Error,
    fmt::{Debug, Display},
    future,
};

/// Maximum payload that can be sent.
pub const MAX_PAYLOAD: usize = stdiomux::mux::basic::MAX_PAYLOAD;

/// Abstract service trait for a herder daemon.
pub trait HerderService<A: HerdAction> {
    type Error: Error;

    async fn start(
        &mut self,
        action: A,
    ) -> Result<
        Started<<A::Event as HerdEvent>::StartInfo, Result<A::Event, Self::Error>>,
        Self::Error,
    >;
}

/// Successfully started a herd.
pub struct Started<S, E> {
    pub start: S,
    pub events: BoxStream<'static, E>,
}

/// Tell the herder to start a herd for performing an arbitrary action.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StartHerd<A> {
    /// ID to associate with all of the herd's events
    pub id: u64,

    /// The action to perform
    pub action: A,
}

/// Trait alias for objects that can be serialized on a wire.
pub trait Message:
    Serialize + DeserializeOwned + Debug + Clone + PartialEq + Send + 'static
{
}

impl<T> Message for T where
    T: Serialize + DeserializeOwned + Debug + Clone + PartialEq + Send + 'static
{
}

/// Arbitrary herd initialization action. This can be anything, from writing to verifying to voiding.
pub trait HerdAction: Message {
    /// The events emitted by the herd afterwards.
    type Event: HerdEvent;
}

/// An event emitted by a running herd.
pub trait HerdEvent: Message + TryFrom<TopLevelHerdEvent, Error = TopLevelHerdEvent> {
    /// The initial information variant that it's expected to send out as soon as it
    /// has started running.
    type StartInfo: Debug + Message;

    /// A failure variant indicating that this herd has terminated unexpectedly and fatally
    /// without any hope of recovery.
    type Failure: Display + Debug + Message;

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

pub fn serialize<'a, T: Serialize + 'a>(value: T) -> Result<Bytes, postcard::Error> {
    let mut out = BytesMut::with_capacity(MAX_PAYLOAD);

    // SAFETY:
    // setting the length is safe because we are filling these bytes before they get read.
    // yes, technically postcard's impl can read the uninitialized data for whatever,
    // but in practice, if you're worried about that, that's kinda your problem lol
    unsafe { out.set_len(MAX_PAYLOAD) };
    postcard::to_slice(&value, &mut out)?;

    Ok(out.freeze())
}

pub fn deserialize<T: DeserializeOwned>(bs: Bytes) -> Result<T, postcard::Error> {
    let (t, _x) = postcard::take_from_bytes(&bs)?;
    Ok(t)
}

/// Encode a [`Started`] into a stream of bytes for the given [`HerdAction`].
pub async fn encode_response<A: HerdAction>(
    response: Started<<A::Event as HerdEvent>::StartInfo, A::Event>,
) -> impl Stream<Item = Bytes> {
    let first = serialize(response.start).expect("failed to serialize first payload");
    let rest = response
        .events
        .map(|e| serialize(e).expect("failed to serialize event"));
    stream::once(future::ready(first)).chain(rest)
}

/// Decode a stream of bytes into a [`Started`] for the given [`HerdAction`].
pub async fn decode_response<A: HerdAction>(
    mut stream: impl Stream<Item = Bytes> + Send + Unpin + 'static,
) -> Result<
    Result<
        Started<<A::Event as HerdEvent>::StartInfo, Result<A::Event, postcard::Error>>,
        <A::Event as HerdEvent>::Failure,
    >,
    postcard::Error,
> {
    let first = stream.next().await.unwrap_or_default();
    let first: Result<<A::Event as HerdEvent>::StartInfo, <A::Event as HerdEvent>::Failure> =
        stdiomux::codec::postcard::deserialize(first)?;

    let start = match first {
        Ok(s) => s,
        Err(e) => return Ok(Err(e)),
    };

    let events: BoxStream<'static, Result<A::Event, ::postcard::Error>> =
        Box::pin(stream.map(|bs| stdiomux::codec::postcard::deserialize(bs)));

    Ok(Ok(Started { start, events }))
}
