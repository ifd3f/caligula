use std::{borrow::Cow, convert::Infallible};

use super::{Frame, Header};
use bytes::{BufMut, Bytes};

/// A simple length framing scheme where values are prefixed by a 32-byte length header.
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

    const MAX_PAYLOAD: usize = 100;
    proptest::collection::vec(proptest::num::u8::ANY, 0..=MAX_PAYLOAD).prop_map(|x| Bytes::from(x))
}

/// Header for [SimpleLengthFraming].
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
        Cow::Owned(SimpleLengthHeader(
            u32::try_from(self.0.len()).expect("Body is too long!"),
        ))
    }

    fn deserialize(_: Self::Header, body: Bytes) -> Result<Self, Self::DeserializeBodyError> {
        Ok(SimpleLengthFrame(body))
    }

    fn serialize(&self, mut buf: &mut [u8]) -> Result<(), Self::SerializeError> {
        buf.put_u32(self.header().0);
        buf.copy_from_slice(&self.0);
        Ok(())
    }
}
