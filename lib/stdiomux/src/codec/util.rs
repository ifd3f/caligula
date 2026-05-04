use std::{
    marker::PhantomData,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

use futures::{
    StreamExt,
    stream::{self, BoxStream},
};
use tokio::{join, sync::mpsc};
use tokio_stream::wrappers::ReceiverStream;

use super::*;

/// Merges an [`Encoder`] and [`Decoder`] together into one [`Codec`].
pub struct DelegateCodec<T, E, D, S> {
    e: E,
    d: D,
    _phantom: PhantomData<(fn(T) -> S, fn(S) -> T)>,
}

impl<'a, T: 'a, E, D, S> DelegateCodec<T, E, D, S>
where
    E: Encoder<'a, T>,
    D: Decoder<'a, T, S>,
    S: Stream<Item = Bytes> + Send + Unpin + 'a,
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
    E: Clone,
    D: Clone,
{
    fn clone(&self) -> Self {
        Self {
            e: self.e.clone(),
            d: self.d.clone(),
            _phantom: PhantomData,
        }
    }
}

impl<'a, T: 'a, E, D, S> Codec<'a, T, S> for DelegateCodec<T, E, D, S>
where
    E: Encoder<'a, T>,
    D: Decoder<'a, T, S>,
    S: Stream<Item = Bytes> + Send + Unpin + 'a,
{
    type Encoder = E;

    type Decoder = D;

    fn encoder(&self) -> Self::Encoder {
        self.e.clone()
    }

    fn decoder(&self) -> Self::Decoder {
        self.d.clone()
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

impl<'a, S> Streamable<'a, S> for S
where
    S: Stream<Item = Bytes> + Send + Unpin + 'a,
{
    type Codec = DelegateCodec<S, ByteStreamEncoder, ByteStreamDecoder, S>;

    fn codec() -> Self::Codec {
        DelegateCodec::<S, ByteStreamEncoder, ByteStreamDecoder, S>::new(
            ByteStreamEncoder,
            ByteStreamDecoder,
        )
    }
}

pub struct OkStream<S>(S);

impl<S> Stream for OkStream<S>
where
    S: Stream + Unpin,
{
    type Item = Result<S::Item, Infallible>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.0.poll_next_unpin(cx).map(|x| x.map(Ok))
    }
}

impl<'a, S> Encoder<'a, S> for ByteStreamEncoder
where
    S: Stream<Item = Bytes> + Send + Unpin + 'a,
{
    type SerializeStream = OkStream<S>;

    type SerializeError = Infallible;

    fn serialize(&self, value: S, _max_payload: usize) -> Self::SerializeStream {
        OkStream(value)
    }
}

impl<'a, S> Decoder<'a, S, S> for ByteStreamDecoder
where
    S: Stream<Item = Bytes> + Send + Unpin + 'a,
{
    type DeserializeFuture = future::Ready<Result<S, Self::DeserializeError>>;

    type DeserializeError = Infallible;

    fn deserialize(&self, bss: S) -> Self::DeserializeFuture {
        future::ready(Ok(bss))
    }
}

/// Construct an encoder from a pure function.
pub fn encoder_fn<'a, F, T, S, E>(f: F) -> FnEncoder<F>
where
    F: Fn(T, usize) -> S + Sync + Send + 'static,
    T: 'a,
    S: Stream<Item = Result<Bytes, E>> + 'a,
    E: Error + 'a,
{
    FnEncoder { f: Arc::new(f) }
}

/// Return result of [`encoder_fn()`].
pub struct FnEncoder<F> {
    f: Arc<F>,
}

impl<F> Clone for FnEncoder<F> {
    fn clone(&self) -> Self {
        Self { f: self.f.clone() }
    }
}

impl<'a, F, T, S, E> Encoder<'a, T> for FnEncoder<F>
where
    F: Fn(T, usize) -> S + Send + Sync + 'static,
    T: 'a,
    S: Stream<Item = Result<Bytes, E>> + 'a,
    E: Error + 'a,
{
    type SerializeStream = S;

    type SerializeError = E;

    fn serialize(&self, value: T, max_payload: usize) -> Self::SerializeStream {
        (self.f)(value, max_payload)
    }
}

/// Construct a decoder from a pure function.
pub fn decoder_fn<'a, F, Fut, T, S, E>(f: F) -> FnDecoder<F>
where
    F: Fn(S) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<T, E>>,
    T: 'a,
    S: Stream<Item = Bytes> + Send + Unpin + 'a,
    E: Error + 'a,
{
    FnDecoder { f: Arc::new(f) }
}

pub struct FnDecoder<F> {
    f: Arc<F>,
}

impl<F> Clone for FnDecoder<F> {
    fn clone(&self) -> Self {
        Self { f: self.f.clone() }
    }
}

impl<'a, F, Fut, T, S, E> Decoder<'a, T, S> for FnDecoder<F>
where
    F: Fn(S) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<T, E>> + Send,
    T: 'a,
    S: Stream<Item = Bytes> + Send + Unpin + 'a,
    E: Error + Send,
{
    type DeserializeFuture = Fut;

    type DeserializeError = E;

    fn deserialize(&self, bss: S) -> Self::DeserializeFuture {
        (self.f)(bss)
    }
}

pub trait EncoderExt<'a, T: 'a>: Encoder<'a, T> {
    /// Applies the given function **before** this encoder.
    fn with<A: 'a, F>(self, f: F) -> impl Encoder<'a, A>
    where
        Self: Sized + 'static,
        F: Fn(A) -> T + Send + Sync + 'static,
    {
        encoder_fn(move |x, max| self.serialize(f(x), max))
    }

    /// Returns an encoder that takes in a `Stream<T>` and serializes it as indivdual values
    /// concatenated with each other.
    ///
    /// WARNING: If this encoder consumes all data, then only the first `T` in the stream will be
    /// serialized!
    fn concat<S>(self) -> impl Encoder<'a, S>
    where
        Self: 'static,
        S: Stream<Item = T> + Unpin + 'a,
    {
        encoder_fn(move |s: S, max_payload: usize| {
            let this = self.clone();
            s.flat_map(move |x| this.serialize(x, max_payload))
        })
    }
}

impl<'a, T: 'a, E> EncoderExt<'a, T> for E where E: Encoder<'a, T> {}

pub trait DecoderExt<'a, T: 'a, S>: Decoder<'a, T, S>
where
    S: Stream<Item = Bytes> + Send + Unpin + 'a,
{
    /// Applies the given function **after** this decoder.
    fn map<A: 'a, F, Fut, E>(self, f: F) -> impl Decoder<'a, A, S, DeserializeError = E>
    where
        Self: Sized + 'static,
        F: Fn(Result<T, Self::DeserializeError>) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<A, E>> + Send,
        E: Error + Send,
    {
        let f = Arc::new(f);
        decoder_fn(move |bss| {
            let this = self.clone();
            let f = f.clone();
            async move { f(this.deserialize(bss).await).await }
        })
    }
}

/// Repeatedly decodes items off of a byte stream into a `Stream<T>`.
///
/// WARNING: If this decoder consumes all data, then only the first `T` in the stream will be
/// deserialized!
pub fn concat<D, T, S>(
    d: D,
) -> impl Decoder<
    'static,
    BoxStream<'static, Result<T, D::DeserializeError>>,
    S,
    DeserializeError = Infallible,
>
where
    D: Decoder<'static, T, ReceiverStream<Bytes>> + 'static,
    D::DeserializeFuture: Send,
    D::DeserializeError: Send,
    T: Send + 'static,
    S: Stream<Item = Bytes> + Send + Unpin + 'static,
{
    let d = Arc::new(d);

    decoder_fn(move |bss: S| {
        let d = d.clone();
        let stream: BoxStream<'static, Result<T, D::DeserializeError>> = Box::pin(stream::unfold(
            // We make bss optional because if the deserializer returned an error,
            // we need to return the error (one cycle), then return None (second cycle).
            (d, Some(bss), None),
            move |(d, bss, unconsumed)| async move {
                // Ensure bss hasn't already been dropped
                let Some(mut bss) = bss else {
                    return None;
                };

                // Problem: deserializer needs to take in a stream, but if we pass our whole stream in,
                // we won't be able to serialize the items afterwards.
                // Solution: A sneaky little trick using mpsc where we still own the actual stream, but
                // feed bytes into the deserializer one at a time.
                let (tx, rx) = mpsc::channel::<Bytes>(1);
                if let Some(unconsumed) = unconsumed {
                    tx.send(unconsumed).await.unwrap();
                }

                // future for generating an item
                let item = d.deserialize(ReceiverStream::new(rx));

                // future for feeding the deserializer
                let feed = async {
                    while let Some(bs) = bss.next().await {
                        // try sending
                        match tx.send(bs).await {
                            Ok(_) => (),                // consumed
                            Err(e) => return Some(e.0), // done consuming
                        }
                    }
                    // out of bytes
                    None
                };

                // drive both in parallel
                let (item, feed) = join!(item, feed);
                match (item, feed) {
                    // done, and there's still more bytes
                    (Ok(item), Some(unconsumed)) => {
                        Some((Ok(item), (d, Some(bss), Some(unconsumed))))
                    }

                    // done, but we didn't feed anything extra in. loop around again in case there's more
                    (Ok(item), None) => Some((Ok(item), (d, Some(bss), None))),

                    // deserializer error is a termination no irregardless of what's left
                    (Err(e), _) => Some((Err(e), (d, None, None))),
                }
            },
        ));

        async move { Ok::<_, Infallible>(stream) }
    })
}
