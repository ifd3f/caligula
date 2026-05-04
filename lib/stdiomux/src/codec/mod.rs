use std::{convert::Infallible, error::Error};

use bytes::Bytes;
use futures::{Stream, future};

pub mod postcard;
pub mod util;

/// An object that can be encoded into a byte stream, or decoded from a byte stream.
pub trait Streamable<S>: Sized
where
    S: Stream<Item = Bytes> + Send + Unpin + 'static,
{
    /// The codec type associated with this object.
    type Codec: Codec<Self, S>;

    /// Construct an instance of the codec.
    fn codec() -> Self::Codec;
}

/// An object that is able to convert types into byte streams.
pub trait Encoder<T>: Clone + Send + Sync + 'static {
    /// The type of byte stream that this codec converts `T` into.
    type SerializeStream: Stream<Item = Bytes> + Send + Unpin + 'static;

    /// An error encountered while deserializing.
    type SerializeError: Error;

    /// Serialize an object into a stream of payloads.
    ///
    /// The maximum payload size is provided as an argument.
    fn serialize(
        &self,
        value: T,
        max_payload: usize,
    ) -> Result<Self::SerializeStream, Self::SerializeError>;
}

/// A type that is able to decode byte streams of type `S` into a given type `T`.
///
/// Notably, `T` may itself be a stream, or contain a stream.
pub trait Decoder<T, S>: Clone + Send + Sync + 'static
where
    S: Stream<Item = Bytes> + Send + Unpin + 'static,
{
    /// The future that [`Self::deserialize`] returns.
    type DeserializeFuture: Future<Output = Result<T, Self::DeserializeError>>;

    /// An error encountered while deserializing.
    type DeserializeError: Error;

    /// Deserialize an object from a byte stream.
    ///
    /// The byte stream may have arbitrary max payload length and the decoder should be able to handle it.
    fn deserialize(&self, bss: S) -> Self::DeserializeFuture;
}

/// Trait alias for types that implement both [`Encoder`] and [`Decoder`].
pub trait Codec<T, S>: Encoder<T> + Decoder<T, S>
where
    S: Stream<Item = Bytes> + Send + Unpin + 'static,
{
}

impl<C, T, S> Codec<T, S> for C
where
    C: Encoder<T> + Decoder<T, S>,
    S: Stream<Item = Bytes> + Send + Unpin + 'static,
{
}
