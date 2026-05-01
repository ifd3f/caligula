use std::{borrow::Cow, convert::Infallible};

use super::{Frame, Header};
use bytes::{Buf as _, BufMut, Bytes};

/// A simple length framing scheme where values are prefixed by a 32-bit length header.
///
/// Methods will panic if the provided body exceeds [u32::MAX]!
#[derive(Debug, PartialEq, Eq, Clone)]
#[cfg_attr(
    feature = "proptest",
    derive(proptest_derive::Arbitrary),
    proptest(params = "()")
)]
pub struct SimpleLengthFrame(
    #[cfg_attr(feature = "proptest", proptest(strategy = "payload_strategy()"))] pub Bytes,
);

#[cfg(feature = "proptest")]
fn payload_strategy() -> impl proptest::prelude::Strategy<Value = Bytes> {
    use proptest::prelude::Strategy as _;

    // slightly over 8 bits to catch endianness bugs
    const MAX_PAYLOAD: usize = 259;
    proptest::collection::vec(proptest::num::u8::ANY, 0..=MAX_PAYLOAD).prop_map(|x| Bytes::from(x))
}

impl SimpleLengthFrame {
    /// Get the length of this frame's body as a [u32]. Panics if too large.
    pub fn body_len(&self) -> u32 {
        u32::try_from(self.0.len()).expect("Body is too large!")
    }
}

/// Header for [SimpleLengthFrame].
#[derive(Debug, PartialEq, Eq, Clone)]
pub struct SimpleLengthHeader(pub u32);

impl Header for SimpleLengthHeader {
    type DeserializeError = Infallible;

    const SIZE: usize = size_of::<u32>();

    fn body_len(&self) -> usize {
        self.0 as usize
    }

    fn deserialize(buf: Bytes) -> Result<Self, Self::DeserializeError> {
        if buf.len() != Self::SIZE {
            panic!("Bad header length!")
        }
        Ok(SimpleLengthHeader(u32::from_be_bytes([
            buf[0], buf[1], buf[2], buf[3],
        ])))
    }
}

impl Frame for SimpleLengthFrame {
    type SerializeError = Infallible;

    type DeserializeBodyError = Infallible;

    type Header = SimpleLengthHeader;

    const MTU: usize = u32::MAX as usize + SimpleLengthHeader::SIZE;

    fn header(&self) -> Cow<'_, Self::Header> {
        Cow::Owned(SimpleLengthHeader(self.body_len()))
    }

    fn deserialize(_: Self::Header, body: Bytes) -> Result<Self, Self::DeserializeBodyError> {
        Ok(SimpleLengthFrame(body))
    }

    fn serialize(self, mut buf: &mut [u8]) -> Result<(), Self::SerializeError> {
        buf.put_u32(self.header().0);
        buf.copy_from_slice(&self.0);
        Ok(())
    }
}

/// A simple mux framing scheme where values are prefixed by a 32-bit header consisting of
/// a 16-bit channel ID and a 16-bit body length.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(
    feature = "proptest",
    derive(proptest_derive::Arbitrary),
    proptest(params = "()")
)]
pub struct SimpleMuxFrame {
    pub channel: u16,
    #[cfg_attr(feature = "proptest", proptest(strategy = "payload_strategy()"))]
    pub body: Bytes,
}

impl SimpleMuxFrame {
    /// Get the length of this frame's body as a u16. Panics if too large.
    pub fn body_len(&self) -> u16 {
        u16::try_from(self.body.len()).expect("Body is too large!")
    }
}

/// Header for [SimpleMuxFrame].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SimpleMuxHeader {
    pub channel: u16,
    pub body_len: u16,
}

impl Header for SimpleMuxHeader {
    type DeserializeError = Infallible;

    const SIZE: usize = 4;

    fn body_len(&self) -> usize {
        self.body_len as usize
    }

    fn deserialize(mut buf: Bytes) -> Result<Self, Self::DeserializeError> {
        let channel = buf.get_u16();
        let body_len = buf.get_u16();
        Ok(Self { channel, body_len })
    }
}

impl Frame for SimpleMuxFrame {
    type SerializeError = Infallible;

    type DeserializeBodyError = Infallible;

    type Header = SimpleMuxHeader;

    const MTU: usize = u16::MAX as usize + SimpleMuxHeader::SIZE;

    fn header(&self) -> Cow<'_, Self::Header> {
        Cow::Owned(SimpleMuxHeader {
            channel: self.channel,
            body_len: self.body_len(),
        })
    }

    fn deserialize(header: Self::Header, body: Bytes) -> Result<Self, Self::DeserializeBodyError> {
        Ok(Self {
            channel: header.channel,
            body,
        })
    }

    fn serialize(self, mut buf: &mut [u8]) -> Result<(), Self::SerializeError> {
        buf.put_u16(self.channel);
        buf.put_u16(self.body_len());
        buf.put(self.body.clone());
        Ok(())
    }
}
