pub mod write;

use futures::{Stream, future::BoxFuture, stream::BoxStream};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::{
    convert::Infallible, error::Error, fmt::Debug, marker::PhantomData, sync::Arc, task::{Context, Poll}
};
use stdiomux::codec::{
    Streamable,
    postcard::{SingleDatagramCodec, SingleDatagramCodecDeserializeFuture},
};
use tower::Service;

/// Maximum payload size that can be sent on the wire.
pub const MAX_PAYLOAD: usize = 4096;

/// Arbitrary herd initialization action. This can be anything, from writing to verifying to voiding.
pub trait HerderAction: Message + Into<TopLevelHerderAction> {
    /// Initial information from the herder.
    type Start: Message;

    /// Errors that the herd can emit.
    type Error: Message + std::error::Error;

    /// The events emitted by the herd after being started.
    type Event: Message;

    /// The final success message.
    type Success: Message;
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum HerdEvent<A: HerderAction> {
    Event(A::Event),
    Success(A::Success),
}

/// Trait alias for things that we support sending over a wire.
pub trait Message:
    Serialize + DeserializeOwned + Debug + Clone + PartialEq + Send + 'static
{
}

impl<T> Message for T where
    T: Serialize + DeserializeOwned + Debug + Clone + PartialEq + Send + 'static
{
}

/// An enum containing all implemented and valid types of herder event.
///
/// This doesn't actually implement [`HerderAction`] because its responses are type-erased
/// on the wire. It exists purely for serialization.
#[derive(
    Debug, Clone, PartialEq, Serialize, Deserialize, derive_more::From, derive_more::TryInto,
)]
#[non_exhaustive]
pub enum TopLevelHerderAction {
    Writer(write::WriteVerifyAction),
}

pub struct HerdStarted<A: HerderAction, E: Error> {
    events: BoxStream<'static, Result<A::Event, E>>,
}

impl<A: HerderAction, E: Error> HerdStarted<A, E> {
    pub fn events(&self) -> impl Stream<'static, Result<HerdEvent<A>, E>> {
        self.events
    }
}

/// Abstract interface to a herder daemon.
pub trait HerderService {
    type Error: std::error::Error;

    fn start_action<A: HerderAction>(
        &self,
        a: A,
    ) -> impl Future<Output = Result<HerdStarted<A, Self::Error>, Self::Error>> + Send + 'static;
}

/// Adapter from [`HerderService`] to [`tower::Service`]
pub struct HerderTowerService<A, S>{
    inner: Arc<S>,
    _phantom: PhantomData<fn(A)>
};

impl<S> Clone for HerderTowerService<S> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<S> From<S> for HerderTowerService<S> {
    fn from(value: S) -> Self {
        HerderTowerService(Arc::new(value))
    }
}

impl<S, A> Service<A> for HerderTowerService<S>
where
    S: HerderService,
    A: HerderAction,
{
    type Response = BoxStream<'static, Result<A::Event, A::Error>>;

    type Error = S::Error;

    type Future = BoxFuture<'static, Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: A) -> Self::Future {
        let fut = self.0.start_action(req);
        Box::pin(fut)
    }
}
