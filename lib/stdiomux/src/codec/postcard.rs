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
pub struct SingleDatagramCodec;

impl<'a, T, S> Codec<'a, T, S> for SingleDatagramCodec
where
    T: Serialize + DeserializeOwned + 'a,
    S: Stream<Item = Bytes> + Send + Unpin + 'a,
{
    type Encoder = SingleDatagramEncoder;

    type Decoder = SingleDatagramDecoder;

    fn encoder(&self) -> Self::Encoder {
        SingleDatagramEncoder
    }

    fn decoder(&self) -> Self::Decoder {
        SingleDatagramDecoder
    }
}

#[derive(Clone)]
pub struct SingleDatagramEncoder;

impl<'a, T: Serialize + 'a> Encoder<'a, T> for SingleDatagramEncoder {
    type SerializeStream = stream::Once<future::Ready<Result<Bytes, Self::SerializeError>>>;

    type SerializeError = ::postcard::Error;

    fn serialize(&self, value: T, max_payload: usize) -> Self::SerializeStream {
        let mut out = BytesMut::with_capacity(max_payload);

        // SAFETY:
        // setting the length is safe because we are filling these bytes before they get read.
        // yes, technically postcard's impl can read the uninitialized data for whatever,
        // but in practice, if you're worried about that, that's kinda your problem lol
        unsafe { out.set_len(max_payload) };
        let r = to_slice(&value, &mut out);

        stream::once(future::ready(match r {
            Ok(_) => Ok(out.freeze()),
            Err(e) => Err(e),
        }))
    }
}

/// A decoder that attempts to serialize and deserialize the input value as a single datagram.
#[derive(Clone)]
pub struct SingleDatagramDecoder;

impl<'a, T: DeserializeOwned + 'a, S> Decoder<'a, T, S> for SingleDatagramDecoder
where
    S: Stream<Item = Bytes> + Send + Unpin + 'a,
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

impl<'a, T: 'a, S> Future for SingleDatagramCodecDeserializeFuture<T, S>
where
    T: DeserializeOwned,
    S: Stream<Item = Bytes> + Send + Unpin + 'a,
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
