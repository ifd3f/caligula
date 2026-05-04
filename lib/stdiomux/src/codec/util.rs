use std::{
    marker::PhantomData,
    pin::Pin,
    task::{Context, Poll},
};

use futures::StreamExt;
use pin_project::pin_project;

use super::*;

/// Trivial identity codec for byte streams.
///
/// WARNING: Does not do any fragmentation if the bytes are bigger than the max payload!
/// Do NOT use if that is a worry!
#[derive(Clone)]
pub struct ByteStreamCodec;

impl<S> Streamable<S> for S
where
    S: Stream<Item = Bytes> + Send + Unpin + 'static,
{
    type Codec = ByteStreamCodec;

    fn codec() -> Self::Codec {
        ByteStreamCodec
    }
}

impl<S> Encoder<S> for ByteStreamCodec
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

impl<S> Decoder<S, S> for ByteStreamCodec
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
        Ok(MapStreamEncoderSerializeStream {
            encoder: self.inner.clone(),
            input: value,
            current: None,
            max_payload,
            _phantom: PhantomData,
        })
    }
}

#[pin_project]
pub struct MapStreamEncoderSerializeStream<E, T, S>
where
    E: Encoder<T>,
{
    encoder: E,
    input: S,
    #[pin]
    current: Option<Pin<Box<E::SerializeStream>>>,
    max_payload: usize,
    _phantom: PhantomData<fn() -> T>,
}

impl<E, T, S> Stream for MapStreamEncoderSerializeStream<E, T, S>
where
    E: Encoder<T>,
    T: Send + 'static,
    S: Stream<Item = T> + Send + Unpin + 'static,
{
    type Item = Bytes;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        loop {
            match self.current.as_mut() {
                Some(c) => match c.poll_next_unpin(cx) {
                    Poll::Ready(Some(bs)) => return Poll::Ready(Some(bs)),
                    Poll::Ready(None) => {
                        self.current = None;
                    }
                    Poll::Pending => return Poll::Pending,
                },
                None => match self.input.poll_next_unpin(cx) {
                    Poll::Ready(Some(new)) => match self.encoder.serialize(new, self.max_payload) {
                        Ok(something) => {
                            self.current = Some(Box::pin(something));
                        }
                        Err(_er) => return Poll::Ready(None),
                    },
                    Poll::Ready(None) => return Poll::Ready(None),
                    Poll::Pending => return Poll::Pending,
                },
            }
        }
    }
}

/*

/// Decoder that maps `Stream<Bytes>` into `Stream<T>`.
pub struct MapStreamDecoder<D, T, S> {
    inner: D,
    _phantom: PhantomData<fn(S) -> T>,
}

impl<D, T, S> From<D> for MapStreamDecoder<D, T, S>
where
    D: Decoder<T, MapStreamDecoderPseudoStream>,
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
            _phantom: self._phantom.clone(),
        }
    }
}

impl<D, T, S> Decoder<MapStreamDecoderSerializeStream<D, T, S>, S> for MapStreamDecoder<D, T, S>
where
    D: Decoder<T, MapStreamDecoderPseudoStream>,
    T: Send + 'static,
    S: Stream<Item = Bytes> + Send + Unpin + 'static,
{
    type DeserializeFuture =
        future::Ready<Result<MapStreamDecoderSerializeStream<D, T, S>, Self::DeserializeError>>;

    type DeserializeError = Infallible;

    fn deserialize(&self, bss: S) -> Self::DeserializeFuture {
        let cell = Arc::new(Mutex::new(None));
        future::ready(Ok(MapStreamDecoderSerializeStream {
            decoder: self.inner.clone(),
            input: bss,
            pseudostream_cell: cell,
            current: None,
            _phantom: PhantomData,
        }))
    }
}

#[pin_project]
pub struct MapStreamDecoderSerializeStream<D, T, S>
where
    D: Decoder<T, MapStreamDecoderPseudoStream>,
    S: Stream<Item = Bytes> + Send + Unpin + 'static,
{
    decoder: D,
    #[pin]
    input: S,
    pseudostream_cell: Arc<Mutex<Option<Bytes>>>,
    current: Option<Pin<Box<D::DeserializeFuture>>>,
    _phantom: PhantomData<fn() -> T>,
}

#[pin_project]
pub struct MapStreamDecoderPseudoStream {
    #[pin]
    cell: Arc<Mutex<Option<Bytes>>>,
    wake: AtomicWaker,
}

impl Stream for MapStreamDecoderPseudoStream {
    type Item = Bytes;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        match self.cell.lock().unwrap().take() {
            Some(x) => Poll::Ready(Some(x)),
            None => {
                self.wake.register(cx.waker());
                Poll::Pending
            }
        }
    }
}

impl<D, T, S> Stream for MapStreamDecoderSerializeStream<D, T, S>
where
    D: Decoder<T, MapStreamDecoderPseudoStream>,
    T: Send + 'static,
    S: Stream<Item = Bytes> + Send + Unpin + 'static,
{
    type Item = Bytes;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        loop {
            match self.current {
                Some(x) => match self.input.poll_next_unpin(cx) {
                    Poll::Ready(r) => todo!(),
                    Poll::Ready(None) => match x.poll_unpin(cx) {
                        Poll::Ready(r) => r.ok().,
                        Poll::Pending => todo!(),
                    },
                    Poll::Pending => todo!(),
                },
                None => {
                    todo!()
                    self.current = Some(Box::pin(self.decoder.deserialize(
                        MapStreamDecoderPseudoStream {
                            cell: self.pseudostream_cell.clone(),
                            wake: AtomicWaker::new(),
                        },
                    )));
                }
            }
        }
    }
}
*/
