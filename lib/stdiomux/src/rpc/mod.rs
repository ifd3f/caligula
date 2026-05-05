use std::{
    marker::PhantomData,
    task::{Context, Poll},
};

use bytes::Bytes;
use futures::{FutureExt, Stream, StreamExt, TryStreamExt, future::BoxFuture};
use tower::{Service, ServiceExt};

use crate::{
    codec::{Codec, Decoder, DecoderOf, Encoder, EncoderOf, Streamable, util::StreamableExt},
    mux::BoxByteStream,
};

pub struct TryStreamShortCircuitFuture<Fut> {
    f:Fut
}

pub struct TryStreamShortCircuitService<S> {
    s: S,
}

pub struct TryStreamShortCircuitResponse<I> {}

pub enum TryStreamShortCircuitError<S, I> {
    Service(S),
    Input(I)
}

impl<I, T> Stream for TryStreamShortCircuitResponse<I>
where
    I: Stream<Item = T>,
{
    type Item = Result<T, TryStreamShortCircuitError<S, I>>;

    fn poll_next(self: std::pin::Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        todo!()
    }
}

impl<S, As, A, NewAs, E> Service<NewAs> for TryStreamShortCircuitService<S>
where
    S: Service<As>,
    S::Response : Stream<Item=A>,
    As: Stream<Item = A>,
    NewAs: Stream<Item = Result<A, E>>,
{
    type Response = TryStreamShortCircuitResponse<As>;

    type Error = S::Error;

    type Future = TryStreamShortCircuitFuture<S::Future>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.s.poll_ready(cx)
    }

    fn call(&mut self, req: NewAs) -> Self::Future {
        let res = self.s.call(req);
        async move {
            let x = res.await;
        }
    }
}

/*
/// A [`Request`] is a [`Streamable`] object that has an associated response.
pub trait Request: for<'a> Streamable<'a, BoxByteStream> {
    type Response: for<'a> Streamable<'a, BoxByteStream>;
}

pub struct RpcClient<S, Req> {
    inner: S,
    max_payload: usize,
    _phantom: PhantomData<fn(Req)>,
}

#[derive(Debug, thiserror::Error)]
pub enum RpcClientError<S, E, D> {
    Service(S),
    Encoder(E),
    Decoder(D),
}

impl<S, Req> Service<Req> for RpcClient<S, Req>
where
    S: Service<BoxByteStream, Response = BoxByteStream>,
    Req: Request + for<'a> StreamableExt<'a, BoxByteStream>,
{
    type Response = Req::Response;

    type Error = RpcClientError<
        S::Error,
        <Req as StreamableExt<'static, BoxByteStream>>::SerializeError,
        <Req as StreamableExt<'static, BoxByteStream>>::DeserializeError,
    >;

    type Future = RpcResponseFuture<Req::Response, Self::Error>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx).map_err(RpcClientError::Service)
    }

    fn call(&mut self, req: Req) -> Self::Future {
        let req_stream = Req::codec().encoder().serialize(req, self.max_payload);

        let fut = Box::pin(async move {
            req_stream.try_filter_map(|x| async move {Ok(Some(x))}).then(|x|)
            let res_stream = self
                .inner
                .call(req_stream)
                .await
                .map_err(RpcClientError::Service)?;
            let decoder = Req::Response::codec()
                .decoder()
                .deserialize(res_stream)
                .await
                .map_err(RpcClientError::Decoder);

            Ok(decoder)
        });
        RpcResponseFuture {
            fut,
            _phantom: PhantomData,
        }
    }
}

pub struct RpcResponseFuture<Res, E> {
    fut: BoxFuture<'static, Result<Res, E>>,
    _phantom: PhantomData<fn(Res)>,
}

impl<Res, E> Future for RpcResponseFuture<Res, E> {
    type Output = Result<Res, E>;

    fn poll(mut self: std::pin::Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.fut.poll_unpin(cx)
    }
}
 */
