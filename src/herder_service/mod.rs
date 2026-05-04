pub mod write;

use futures::{future::BoxFuture, stream::BoxStream};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::{
    convert::Infallible,
    fmt::Debug,
    sync::Arc,
    task::{Context, Poll},
};
use tower::Service;

/// Arbitrary herd initialization action. This can be anything, from writing to verifying to voiding.
pub trait HerderAction: Message {
    /// Errors that the herd can emit.
    type Error: Message + std::error::Error;

    /// The events emitted by the herd after being started.
    type Event: Message;
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, derive_more::From)]
#[non_exhaustive]
pub enum TopLevelHerderAction {
    Writer(write::WriteVerifyAction),
}

/// Abstract interface to a herder daemon.
pub trait HerderService<A: HerderAction> {
    fn start_action(
        &self,
        a: A,
    ) -> impl Future<Output = BoxStream<'static, Result<A::Event, A::Error>>> + Send + 'static;
}

/// Adapter from [`HerderService`] to [`tower::Service`]
pub struct HerderTowerService<S>(Arc<S>);

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
    S: HerderService<A>,
    A: HerderAction,
{
    type Response = BoxStream<'static, Result<A::Event, A::Error>>;

    type Error = Infallible;

    type Future = BoxFuture<'static, Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: A) -> Self::Future {
        let fut = self.0.start_action(req);
        Box::pin(async move { Ok(fut.await) })
    }
}
