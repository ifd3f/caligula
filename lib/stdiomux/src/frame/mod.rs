#[cfg(feature = "io-std")]
pub mod sync;

use std::{error::Error, fmt::Debug};

use bytes::Bytes;

use crate::util::Sealed;

/// A frame that can be serialized and deserialized onto a wire.
///
/// A frame is composed of a fixed-size [Header], followed by an arbitrarily long body.
///
/// Serialization occurs in a single phase.
///
/// Deserialization occurs in two phases: the header must be read first, then the
/// body must be read based on length inferred from the header.
#[expect(private_bounds)]
pub trait Frame: Sized + Clone + Debug + PartialEq + Eq + Sealed {
    /// Errors encountered while serializing this frame onto a wire.
    type SerializeError: Error;

    /// Errors encountered while deserializing the body of this frame from a wire.
    type DeserializeBodyError: Error;

    /// The header of this frame.
    type Header: Header;

    /// Absolute maximum size of a frame, INCLUDING header.
    const MTU: usize;

    /// Get a reference to the header of this frame.
    fn header(&self) -> &Self::Header;

    /// Deserialize the body of this frame, given a header. Body will be empty if the
    /// header indicated a zero-length body.
    ///
    /// Panics if the length of the provided body is NOT equal to [`Header::body_len()`].
    fn deserialize(header: Self::Header, body: Bytes) -> Result<Self, Self::DeserializeBodyError>;

    /// Serialize this frame, including the header, into the given buffer.
    ///
    /// Panics if the provided buffer length is NOT `Self::Header::SIZE + self.header().body_len()` bytes.
    fn serialize(&self, buf: &mut [u8]) -> Result<(), Self::SerializeError>;
}

/// The header of a [Frame].
#[expect(private_bounds)]
pub trait Header: Sized + Clone + Debug + PartialEq + Eq + Sealed {
    /// Errors encountered while deserializing this header from a wire.
    type DeserializeError: Error;

    /// Size of the header. This must be a constant size.
    const SIZE: usize;

    /// How long the body following this header is.
    fn body_len(&self) -> usize;

    /// Attempt to deserialize this from a buffer.
    ///
    /// Panics if the provided buffer is not exactly [Self::SIZE].
    fn deserialize(buf: Bytes) -> Result<Self, Self::DeserializeError>;
}

/// A [Frame] that is a strictly fixed size.
///
/// Implementors get [Frame] and [Header] for free. When deserializing, the frame
/// will be deserialized in the header phase and skip the body phase entirely.
#[expect(private_bounds)]
pub trait FixedSizeFrame: Sized + Clone + Debug + PartialEq + Eq + Sealed {
    /// Errors encountered while serializing this frame onto a wire.
    type SerializeError: Error;

    /// Errors encountered while deserializing this frame from a wire.
    type DeserializeError: Error;

    /// Size of the frame.
    const SIZE: usize;

    /// Serialize this frame into the given buffer.
    ///
    /// Panics if the provided buffer is not exactly [Self::SIZE].
    fn serialize(&self, buf: &mut [u8]) -> Result<(), Self::SerializeError>;

    /// Attempt to deserialize this from a buffer.
    ///
    /// Panics if the provided buffer is not exactly [Self::SIZE].
    fn deserialize(buf: Bytes) -> Result<Self, Self::DeserializeError>;
}

impl<F: FixedSizeFrame> Header for F {
    type DeserializeError = <Self as FixedSizeFrame>::DeserializeError;

    const SIZE: usize = <Self as FixedSizeFrame>::SIZE;

    #[inline]
    fn body_len(&self) -> usize {
        0
    }

    #[inline]
    fn deserialize(buf: Bytes) -> Result<Self, Self::DeserializeError> {
        <Self as FixedSizeFrame>::deserialize(buf)
    }
}

impl<F: FixedSizeFrame> Frame for F {
    type SerializeError = <Self as FixedSizeFrame>::SerializeError;

    type DeserializeBodyError = <Self as FixedSizeFrame>::DeserializeError;

    type Header = Self;

    const MTU: usize = <Self as FixedSizeFrame>::SIZE;

    #[inline]
    fn header(&self) -> &Self::Header {
        self
    }

    #[inline]
    fn deserialize(this: Self::Header, bs: Bytes) -> Result<Self, Self::DeserializeBodyError> {
        if !bs.is_empty() {
            panic!("Expected a zero-sized body length!")
        }
        Ok(this)
    }

    #[inline]
    fn serialize(&self, buf: &mut [u8]) -> Result<(), Self::SerializeError> {
        <Self as FixedSizeFrame>::serialize(self, buf)
    }
}
