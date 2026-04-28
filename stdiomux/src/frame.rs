use std::ffi::CStr;

use byte_strings::const_concat_bytes;
#[cfg(test)]
use bytes::Bytes;
use bytes::{Buf, BufMut, BytesMut, TryGetError};
use strum::{EnumDiscriminants, FromRepr, IntoDiscriminant};

#[cfg(test)]
use proptest::prelude::*;

const MAX_PAYLOAD: usize = 4096;

#[expect(clippy::transmute_ptr_to_ref, reason = "const_concat_bytes! emits this unfortunately")]
const HELLO_PAYLOAD: &[u8; MAX_PAYLOAD] = {
    const MAGIC: &[u8] = concat!("stdiomux piped v", env!("CARGO_PKG_VERSION"), "\0").as_bytes();
    const_concat_bytes!(MAGIC, &[0u8; MAX_PAYLOAD - MAGIC.len()])
};

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Clone, Copy)]
#[cfg_attr(test, derive(proptest_derive::Arbitrary))]
pub struct ChannelId(#[cfg_attr(test, proptest(strategy = "0u16..=65535"))] pub u16);

#[derive(Debug, PartialEq, Eq, Clone, EnumDiscriminants)]
#[cfg_attr(test, derive(proptest_derive::Arbitrary), proptest(params = "()"))]
#[strum_discriminants(name(FrameType))]
#[strum_discriminants(derive(FromRepr))]
#[strum_discriminants(repr(u8))]
pub enum Frame {
    MuxControl(MuxControlFrame),
    ChannelControl(ChannelId, ChannelControlFrame),
    ChannelData(ChannelId, ChannelDataFrame),
}

impl Frame {}

#[derive(Debug, PartialEq, Eq, Clone, EnumDiscriminants)]
#[cfg_attr(test, derive(proptest_derive::Arbitrary), proptest(params = "()"))]
#[strum_discriminants(name(MuxControlOpcode))]
#[strum_discriminants(derive(FromRepr))]
#[strum_discriminants(repr(u8))]
pub enum MuxControlFrame {
    Reset,
    Hello,
    Terminate,
    Finished,
}

#[derive(Debug, PartialEq, Eq, Clone, EnumDiscriminants)]
#[cfg_attr(test, derive(proptest_derive::Arbitrary), proptest(params = "()"))]
#[strum_discriminants(name(ChannelControlOpcode))]
#[strum_discriminants(derive(FromRepr))]
#[strum_discriminants(repr(u8))]
pub enum ChannelControlFrame {
    Reset,
    Open(u8),
    Admit(u8),
}

#[derive(Debug, PartialEq, Eq, Clone)]
#[cfg_attr(test, derive(proptest_derive::Arbitrary), proptest(params = "()"))]
pub struct ChannelDataFrame(
    #[cfg_attr(test, proptest(strategy = "payload_strategy()"))] pub bytes::Bytes,
);

#[cfg(test)]
pub fn payload_strategy() -> impl Strategy<Value = Bytes> {
    proptest::collection::vec(proptest::num::u8::ANY, 0..=MAX_PAYLOAD).prop_map(|x| Bytes::from(x))
}

#[derive(Debug, thiserror::Error)]
pub enum SerializeFrameError {
    #[error("Frame payload is too long")]
    FrameTooLong,
}

pub fn serialize_frame(mut dst: impl BufMut, frame: &Frame) -> Result<(), SerializeFrameError> {
    // Write the header
    let frame_type = frame.discriminant() as u32;
    let argument = match frame {
        Frame::MuxControl(f) => {
            let opcode = f.discriminant() as u32;
            opcode << 24
        }
        Frame::ChannelControl(id, f) => {
            let opcode = f.discriminant() as u32;
            let argument: u8 = match f {
                ChannelControlFrame::Reset => 0,
                ChannelControlFrame::Open(size) => *size,
                ChannelControlFrame::Admit(permits) => *permits,
            };
            opcode << 24 | (argument as u32) << 16 | id.0 as u32
        }
        Frame::ChannelData(id, f) => {
            if f.0.len() > MAX_PAYLOAD {
                return Err(SerializeFrameError::FrameTooLong);
            }
            (f.0.len() as u32) << 16 | id.0 as u32
        }
    };
    let header = frame_type << 28 | argument;
    dst.put_u32(header);

    match frame {
        Frame::MuxControl(f) => match f {
            MuxControlFrame::Hello => dst.put(HELLO_PAYLOAD.as_slice()),
            MuxControlFrame::Reset | MuxControlFrame::Terminate | MuxControlFrame::Finished => (),
        },
        Frame::ChannelControl(_, f) => match f {
            ChannelControlFrame::Reset
            | ChannelControlFrame::Open(_)
            | ChannelControlFrame::Admit(_) => (),
        },
        Frame::ChannelData(_, f) => dst.put(f.0.clone()),
    }

    Ok(())
}

#[derive(Debug, thiserror::Error)]
pub enum DeserializeFrameError {
    #[error("Invalid frame type {0}")]
    InvalidFrameType(u8),
    #[error("Invalid hello {0:?}")]
    InvalidHello(String),
    #[error("Invalid mux control opcode {0}")]
    InvalidMuxControlOpcode(u8),
    #[error("Invalid channel control opcode {0}")]
    InvalidChannelControlOpcode(u8),
    #[error("Ran out of data")]
    OutOfData(#[from] TryGetError),
}

pub fn deserialize_frame(mut src: impl Buf) -> Result<Frame, DeserializeFrameError> {
    let header = src.try_get_u32()?;
    let frame_type = (header >> 28) as u8;
    let frame_type = FrameType::from_repr(frame_type)
        .ok_or(DeserializeFrameError::InvalidFrameType(frame_type))?;

    let argument = header & 0x0fffffff; // mask out top 4 bits
    Ok(match frame_type {
        FrameType::MuxControl => {
            let opcode = (argument >> 24) as u8;
            let opcode = MuxControlOpcode::from_repr(opcode)
                .ok_or(DeserializeFrameError::InvalidMuxControlOpcode(opcode))?;
            Frame::MuxControl(match opcode {
                MuxControlOpcode::Hello => {
                    let mut payload = vec![0u8; MAX_PAYLOAD];
                    src.copy_to_slice(&mut payload);
                    if payload != HELLO_PAYLOAD {
                        let cstr = CStr::from_bytes_until_nul(&payload)
                            .map(|s| s.to_string_lossy())
                            .unwrap_or_else(|_| String::from_utf8_lossy(&payload));
                        Err(DeserializeFrameError::InvalidHello(cstr.to_string()))?
                    }
                    MuxControlFrame::Hello
                }
                MuxControlOpcode::Reset => MuxControlFrame::Reset,
                MuxControlOpcode::Terminate => MuxControlFrame::Terminate,
                MuxControlOpcode::Finished => MuxControlFrame::Finished,
            })
        }
        FrameType::ChannelControl => {
            let opcode = (argument >> 24) as u8;
            let opcode: ChannelControlOpcode = ChannelControlOpcode::from_repr(opcode)
                .ok_or(DeserializeFrameError::InvalidChannelControlOpcode(opcode))?;
            let channel_argument = (argument >> 16) as u8;
            let channel_id = ChannelId((argument & 0xffff) as u16);

            Frame::ChannelControl(
                channel_id,
                match opcode {
                    ChannelControlOpcode::Reset => ChannelControlFrame::Reset,
                    ChannelControlOpcode::Open => ChannelControlFrame::Open(channel_argument),
                    ChannelControlOpcode::Admit => ChannelControlFrame::Admit(channel_argument),
                },
            )
        }
        FrameType::ChannelData => {
            let length = (argument >> 16) as usize;
            let channel_id = ChannelId((argument & 0xffff) as u16);
            let mut payload = BytesMut::zeroed(length);
            src.copy_to_slice(&mut payload);
            Frame::ChannelData(channel_id, ChannelDataFrame(payload.freeze()))
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    proptest! {
        #[test]
        fn test_serialize_roundtrip(frame in any_with::<Frame>(())) {
            let mut buf = vec![];
            serialize_frame(&mut buf, &frame).unwrap();
            println!("serialized header: {:032b}", u32::from_be_bytes([buf[0], buf[1], buf[2], buf[3]]));

            let mut cursor = &buf[..];
            let result = deserialize_frame(&mut cursor).unwrap();

            assert_eq!(result, frame, "not the same frame");
            assert!(cursor.is_empty(), "did not consume all produced data");
        }
    }
}
