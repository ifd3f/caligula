use std::{
    marker::PhantomData,
    sync::Arc,
    task::{Context, Poll},
};

use futures::{future::BoxFuture, stream::BoxStream};
use tower::Service;

use crate::herder_api::HerdEvent;

use super::{HerdAction, HerderService, Started};

/// Adapter from [`HerderService`] to [`tower::Service`]
pub struct HerderTowerService<A, S> {
    inner: Arc<S>,
    _phantom: PhantomData<fn(A)>,
}

impl<A, S> Clone for HerderTowerService<A, S> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            _phantom: PhantomData,
        }
    }
}

impl<A, S> From<S> for HerderTowerService<A, S> {
    fn from(value: S) -> Self {
        Self {
            inner: Arc::new(value),
            _phantom: PhantomData,
        }
    }
}

impl<A, S> Service<A> for HerderTowerService<A, S>
where
    A: HerdAction,
    S: HerderService<A> + Send + Sync + 'static,
{
    type Response = Started<<A::Event as HerdEvent>::StartInfo, Result<A::Event, Self::Error>>;

    type Error = S::Error;

    type Future = BoxFuture<'static, Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: A) -> Self::Future {
        let fut = async move {
            let res = self.inner.start(req).await?;

            Ok(Started {
                start: res.start,
                events: res.events,
            })
        };
        Box::pin(fut)
    }
}
