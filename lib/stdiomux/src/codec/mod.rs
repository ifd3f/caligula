use std::error::Error;

use bytes::Bytes;
use futures::Stream;

/// An object that can be encoded into a byte stream, or decoded from a byte stream.
pub trait Streamable: Sized {
    /// The codec type associated with this object.
    type Codec: Codec<Self>;

    /// Construct an instance of the codec.
    fn codec() -> Self::Codec;
}

/// A codec that is able to convert types into byte streams.
pub trait Encoder<T> {
    /// The future that [`Self::serialize`] returns.
    type SerializeFuture: Future<Output = Result<Self::SerializeStream, Self::SerializeError>>;

    /// The type of byte stream that this codec converts `T` into.
    type SerializeStream: Stream<Item = Bytes>;

    /// An error encountered while deserializing.
    type SerializeError: Error;

    /// Serialize an object into a stream of payloads. The maximum payload size
    /// is provided as an argument.
    fn serialize(&self, value: T, max_payload: usize) -> Self::SerializeStream;
}

/// A type that is able to decode byte streams into encoders.
pub trait Decoder<T> {
    /// The future that [`Self::deserialize`] returns.
    type DeserializeFuture: Future<Output = Result<T, Self::DeserializeError>>;

    /// An error encountered while deserializing.
    type DeserializeError: Error;

    /// Deserialize an object from a byte stream. The byte stream may have arbitrary
    /// max payload length and the decoder should be able to handle it.
    fn deserialize(&self) -> Self::DeserializeFuture;
}

/// Trait alias for types that implement both [`Encoder`] and [`Decoder`].
pub trait Codec<T>: Encoder<T> + Decoder<T> {}

impl<C, T> Codec<T> for C where C: Encoder<T> + Decoder<T> {}
