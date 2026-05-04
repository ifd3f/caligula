//! Traits and helpers for working with datagram framing.
//!
//! WARNING: This is meant to be a purely internally-used library! There are no stability
//! guarantees! End users of this library should not be implementing these traits or using
//! these types directly!

pub mod simple;
pub mod sync;
pub mod tokio;

use std::{borrow::Cow, error::Error, fmt::Debug};

use bytes::Bytes;

/// A frame that can be serialized and deserialized onto a wire.
///
/// A frame is composed of a fixed-size [Header], followed by an arbitrarily long body.
///
/// Serialization occurs in a single phase.
///
/// Deserialization occurs in two phases: the header must be read first, then the
/// body must be read based on length inferred from the header.
///
/// WARNING: All serializers and deserializers will operate on these frames in memory!
/// You should make your frames reasonably sized!
pub trait Frame: Sized + Clone + Debug + PartialEq + Eq {
    /// Errors encountered while serializing this frame onto a wire.
    type SerializeError: Error;

    /// Errors encountered while deserializing the body of this frame from a wire.
    type DeserializeBodyError: Error;

    /// The header of this frame.
    type Header: Header;

    /// Absolute maximum size of a frame, INCLUDING header.
    const MTU: usize;

    /// Borrow or calculate the header of this frame.
    fn header(&self) -> Cow<'_, Self::Header>;

    /// Deserialize the body of this frame, given a header. Body will be empty if the
    /// header indicated a zero-length body.
    ///
    /// Panics if the length of the provided body is NOT equal to [`Header::body_len()`].
    fn deserialize(header: Self::Header, body: Bytes) -> Result<Self, Self::DeserializeBodyError>;

    /// Serialize this frame, including the header, into the given buffer.
    ///
    /// Panics if the provided buffer length is NOT `Self::Header::SIZE + self.header().body_len()` bytes.
    fn serialize(self, buf: &mut [u8]) -> Result<(), Self::SerializeError>;
}

/// The header of a [Frame].
pub trait Header: Sized + Clone + Debug + PartialEq + Eq {
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
pub trait FixedSizeFrame: Sized + Clone + Debug + PartialEq + Eq {
    /// Errors encountered while serializing this frame onto a wire.
    type SerializeError: Error;

    /// Errors encountered while deserializing this frame from a wire.
    type DeserializeError: Error;

    /// Size of the frame.
    const SIZE: usize;

    /// Serialize this frame into the given buffer.
    ///
    /// Panics if the provided buffer is not exactly [Self::SIZE].
    fn serialize(self, buf: &mut [u8]) -> Result<(), Self::SerializeError>;

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
    fn header(&self) -> Cow<'_, Self::Header> {
        Cow::Borrowed(self)
    }

    #[inline]
    fn deserialize(this: Self::Header, bs: Bytes) -> Result<Self, Self::DeserializeBodyError> {
        if !bs.is_empty() {
            panic!("Expected a zero-sized body length!")
        }
        Ok(this)
    }

    #[inline]
    fn serialize(self, buf: &mut [u8]) -> Result<(), Self::SerializeError> {
        <Self as FixedSizeFrame>::serialize(self, buf)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum WriteFrameError<F: Frame> {
    #[error("Error serializing frame: {0}")]
    Frame(F::SerializeError),
    #[error("I/O error: {0}")]
    IO(#[from] std::io::Error),
}

#[derive(Debug, thiserror::Error)]
pub enum ReadFrameError<F: Frame> {
    #[error("Error reading header: {0}")]
    Header(<F::Header as Header>::DeserializeError),
    #[error("Error reading frame: {0}")]
    Body(F::DeserializeBodyError),
    #[error("I/O error: {0}")]
    IO(#[from] std::io::Error),
}
