use std::{
    marker::PhantomData,
    pin::Pin,
    task::{Context, Poll},
};

use futures::{
    StreamExt,
    stream::{BoxStream, unfold},
};

use crate::mux::BoxByteStream;

use super::*;

/// Merges an [`Encoder`] and [`Decoder`] together into one [`Codec`].
pub struct DelegateCodec<T, E, D, S> {
    e: E,
    d: D,
    _phantom: PhantomData<fn(S) -> T>,
}

impl<T, E, D, S> DelegateCodec<T, E, D, S>
where
    E: Encoder<T>,
    D: Decoder<T, S>,
    S: Stream<Item = Bytes> + Send + Unpin + 'static,
{
    pub fn new(e: E, d: D) -> Self {
        Self {
            e,
            d,
            _phantom: PhantomData,
        }
    }
}

impl<T, E, D, S> Clone for DelegateCodec<T, E, D, S>
where
    E: Encoder<T>,
    D: Decoder<T, S>,
    S: Stream<Item = Bytes> + Send + Unpin + 'static,
{
    fn clone(&self) -> Self {
        Self {
            e: self.e.clone(),
            d: self.d.clone(),
            _phantom: PhantomData,
        }
    }
}

impl<T, E, D, S> Encoder<T> for DelegateCodec<T, E, D, S>
where
    T: 'static,
    E: Encoder<T>,
    D: Decoder<T, S>,
    S: Stream<Item = Bytes> + Send + Unpin + 'static,
{
    type SerializeStream = E::SerializeStream;

    type SerializeError = E::SerializeError;

    fn serialize(
        &self,
        value: T,
        max_payload: usize,
    ) -> Result<Self::SerializeStream, Self::SerializeError> {
        self.e.serialize(value, max_payload)
    }
}

impl<T, E, D, S> Decoder<T, S> for DelegateCodec<T, E, D, S>
where
    T: 'static,
    E: Encoder<T>,
    D: Decoder<T, S>,
    S: Stream<Item = Bytes> + Send + Unpin + 'static,
{
    type DeserializeFuture = D::DeserializeFuture;

    type DeserializeError = D::DeserializeError;

    fn deserialize(&self, bss: S) -> Self::DeserializeFuture {
        self.d.deserialize(bss)
    }
}

/// Trivial identity encoder for byte streams.
///
/// WARNING: Does not do any fragmentation if the bytes are bigger than the max payload!
/// Do NOT use if that is a worry!
#[derive(Clone)]
pub struct ByteStreamEncoder;

/// Trivial identity decoder for byte streams.
#[derive(Clone)]
pub struct ByteStreamDecoder;

impl<S> Streamable<S> for S
where
    S: Stream<Item = Bytes> + Send + Unpin + 'static,
{
    type Codec = DelegateCodec<S, ByteStreamEncoder, ByteStreamDecoder, S>;

    fn codec() -> Self::Codec {
        DelegateCodec::<S, ByteStreamEncoder, ByteStreamDecoder, S>::new(
            ByteStreamEncoder,
            ByteStreamDecoder,
        )
    }
}

impl<S> Encoder<S> for ByteStreamEncoder
where
    S: Stream<Item = Bytes> + Send + Unpin + 'static,
{
    type SerializeStream = S;

    type SerializeError = Infallible;

    fn serialize(
        &self,
        value: S,
        _max_payload: usize,
    ) -> Result<Self::SerializeStream, Self::SerializeError> {
        Ok(value)
    }
}

impl<S> Decoder<S, S> for ByteStreamDecoder
where
    S: Stream<Item = Bytes> + Send + Unpin + 'static,
{
    type DeserializeFuture = future::Ready<Result<S, Self::DeserializeError>>;

    type DeserializeError = Infallible;

    fn deserialize(&self, bss: S) -> Self::DeserializeFuture {
        future::ready(Ok(bss))
    }
}

/// Encoder that maps `Stream<T>` into `Stream<Bytes>`.
pub struct MapStreamEncoder<E, T> {
    inner: E,
    _phantom: PhantomData<fn() -> T>,
}

impl<E, T> Clone for MapStreamEncoder<E, T>
where
    E: Encoder<T>,
{
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            _phantom: PhantomData,
        }
    }
}

impl<E, T> From<E> for MapStreamEncoder<E, T>
where
    E: Encoder<T>,
    T: Send + 'static,
{
    fn from(encoder: E) -> Self {
        Self {
            inner: encoder,
            _phantom: PhantomData,
        }
    }
}

impl<E, T, S> Encoder<S> for MapStreamEncoder<E, T>
where
    E: Encoder<T>,
    T: Send + 'static,
    S: Stream<Item = T> + Send + Unpin + 'static,
{
    type SerializeStream = MapStreamEncoderSerializeStream<E, T, S>;

    type SerializeError = Infallible;

    fn serialize(
        &self,
        value: S,
        max_payload: usize,
    ) -> Result<Self::SerializeStream, Self::SerializeError> {
        let encoder = self.inner.clone();
        Ok(MapStreamEncoderSerializeStream {
            inner: Box::pin(
                value
                    .filter_map(move |x| {
                        let encoder = encoder.clone();
                        async move { encoder.serialize(x, max_payload).ok() }
                    })
                    .flatten(),
            ),
            _phantom: PhantomData,
        })
    }
}

pub struct MapStreamEncoderSerializeStream<E, T, S>
where
    E: Encoder<T>,
    S: Stream<Item = T> + Send + Unpin + 'static,
{
    inner: BoxByteStream,
    _phantom: PhantomData<fn(E, S) -> T>,
}

impl<E, T, S> Stream for MapStreamEncoderSerializeStream<E, T, S>
where
    E: Encoder<T>,
    T: Send + 'static,
    S: Stream<Item = T> + Send + Unpin + 'static,
{
    type Item = Bytes;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.inner.poll_next_unpin(cx)
    }
}

/// Decoder that maps `Stream<Bytes>` into `Stream<T>`.
pub struct MapStreamDecoder<D, T, S> {
    inner: D,
    _phantom: PhantomData<fn(S) -> T>,
}

impl<D, T, S> From<D> for MapStreamDecoder<D, T, S>
where
    D: Decoder<T, S>,
    T: Send + 'static,
    S: Stream<Item = Bytes> + Send + Unpin + 'static,
{
    fn from(value: D) -> Self {
        Self {
            inner: value,
            _phantom: PhantomData,
        }
    }
}
impl<D, T, S> Clone for MapStreamDecoder<D, T, S>
where
    D: Clone,
{
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            _phantom: PhantomData,
        }
    }
}

impl<D, T, S> Decoder<MapStreamDecoderSerializeStream<D, T, S>, S> for MapStreamDecoder<D, T, S>
where
    D: Decoder<T, S>,
    T: Send + 'static,
    S: Stream<Item = Bytes> + Send + Unpin + 'static,
{
    type DeserializeFuture =
        future::Ready<Result<MapStreamDecoderSerializeStream<D, T, S>, Self::DeserializeError>>;

    type DeserializeError = Infallible;

    fn deserialize(&self, bss: S) -> Self::DeserializeFuture {
        let decoder = self.inner.clone();
        let fut = unfold(bss, move |mut bss| {
            let decoder = decoder.clone();
            async move {
                let substream = unfold(&mut bss, |bss| async move {
                    bss.next().await.map(|bs| (bs, bss))
                });
                let Ok(x) = decoder.deserialize(Box::pin(substream)).await else {
                    return None;
                };

                Some((x, bss))
            }
        });

        future::ready(Ok(MapStreamDecoderSerializeStream {
            inner: Box::pin(fut),
            _phantom: PhantomData,
        }))
    }
}

pub struct MapStreamDecoderSerializeStream<D, T, S>
where
    D: Decoder<T, S>,
    S: Stream<Item = Bytes> + Send + Unpin + 'static,
{
    inner: BoxStream<'static, T>,
    _phantom: PhantomData<fn(D, S) -> T>,
}

impl<D, T, S> Stream for MapStreamDecoderSerializeStream<D, T, S>
where
    D: Decoder<T, S>,
    T: Send + 'static,
    S: Stream<Item = Bytes> + Send + Unpin + 'static,
{
    type Item = T;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.inner.poll_next_unpin(cx)
    }
}
