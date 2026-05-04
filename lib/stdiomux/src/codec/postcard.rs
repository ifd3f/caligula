//! Encoding and decoding using the [::postcard] crate.

use std::{
    future,
    marker::PhantomData,
    pin::Pin,
    task::{Context, Poll},
};

use ::postcard::{take_from_bytes, to_slice};
use bytes::BytesMut;
use futures::{StreamExt, stream};
use pin_project::pin_project;
use serde::{Serialize, de::DeserializeOwned};

use super::*;

/// A codec that attempts to serialize and deserialize the input value as a single datagram.
#[derive(Clone)]
pub struct SingleDatagramCodec;

impl<T: Serialize> Encoder<T> for SingleDatagramCodec {
    type SerializeStream = stream::Once<future::Ready<Bytes>>;

    type SerializeError = ::postcard::Error;

    fn serialize(
        &self,
        value: T,
        max_payload: usize,
    ) -> Result<Self::SerializeStream, Self::SerializeError> {
        let mut out = BytesMut::with_capacity(max_payload);

        // SAFETY:
        // setting the length is safe because we are filling these bytes before they get read.
        // yes, technically postcard's impl can read the uninitialized data for whatever,
        // but in practice, if you're worried about that, that's kinda your problem lol
        unsafe { out.set_len(max_payload) };
        to_slice(&value, &mut out)?;

        Ok(stream::once(future::ready(out.freeze())))
    }
}

impl<T, S> Decoder<T, S> for SingleDatagramCodec
where
    T: DeserializeOwned,
    S: Stream<Item = Bytes> + Send + Unpin + 'static,
{
    type DeserializeFuture = SingleDatagramCodecDeserializeFuture<T, S>;

    type DeserializeError = ::postcard::Error;

    fn deserialize(&self, bss: S) -> Self::DeserializeFuture {
        SingleDatagramCodecDeserializeFuture::<T, S> {
            bss,
            _phantom: PhantomData,
        }
    }
}

#[pin_project]
pub struct SingleDatagramCodecDeserializeFuture<T, S> {
    #[pin]
    bss: S,
    _phantom: PhantomData<fn() -> T>,
}

impl<T, S> Future for SingleDatagramCodecDeserializeFuture<T, S>
where
    T: DeserializeOwned,
    S: Stream<Item = Bytes> + Send + Unpin + 'static,
{
    type Output = Result<T, ::postcard::Error>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.project().bss.poll_next_unpin(cx).map(|bs| {
            let bs = bs.unwrap_or_default();
            let (t, _x) = take_from_bytes(&bs)?;
            Ok(t)
        })
    }
}
