use std::{convert::Infallible, error::Error};

use bytes::Bytes;
use futures::{Stream, future};

pub mod postcard;
pub mod util;

/// An object that can be encoded into a byte stream of arbitrary encoder-defined type,
/// or decoded from a byte stream of type `S`, using a designated [`Codec`].
pub trait Streamable<'a, S>: Sized + 'a
where
    S: Stream<Item = Bytes> + Send + Unpin + 'a,
{
    /// The codec type associated with this object.
    type Codec: Codec<'a, Self, S>;

    /// Construct an instance of the associated codec.
    fn codec() -> Self::Codec;
}

/// Combined [`Encoder`] and [`Decoder`].
pub trait Codec<'a, T: 'a, S>
where
    S: Stream<Item = Bytes> + Send + Unpin + 'a,
{
    type Encoder: Encoder<'a, T>;
    type Decoder: Decoder<'a, T, S>;

    fn encoder(&self) -> Self::Encoder;
    fn decoder(&self) -> Self::Decoder;
}

/// An object that is able to convert `T`s into byte streams.
pub trait Encoder<'a, T: 'a>: Clone + Send + Sync {
    /// The type of byte stream that this codec converts `T` into.
    type SerializeStream: Stream<Item = Result<Bytes, Self::SerializeError>> + 'a;

    /// An error encountered while deserializing.
    type SerializeError: Error + 'a;

    /// Serialize an object into a stream of payloads.
    ///
    /// The maximum payload size is provided as an argument.
    fn serialize(&self, value: T, max_payload: usize) -> Self::SerializeStream;
}

/// A type that is able to decode byte streams of type `S` into a given type `T`.
///
/// Notably, `T` may itself be a stream, or contain a stream.
pub trait Decoder<'a, T: 'a, S>: Clone + Send + Sync
where
    S: Stream<Item = Bytes> + Send + Unpin + 'a,
{
    /// The future that [`Self::deserialize`] returns.
    type DeserializeFuture: Future<Output = Result<T, Self::DeserializeError>> + Send;

    /// An error encountered while deserializing.
    type DeserializeError: Error + Send;

    /// Deserialize an object from a byte stream.
    ///
    /// The byte stream may have arbitrary max payload length and the decoder should be able to handle it.
    fn deserialize(&self, bss: S) -> Self::DeserializeFuture;
}
