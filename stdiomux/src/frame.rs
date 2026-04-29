use std::{ffi::CStr, io::ErrorKind};

use byte_strings::const_concat_bytes;
use bytes::{BufMut, Bytes, BytesMut};
use strum::{EnumDiscriminants, FromRepr, IntoDiscriminant};

#[cfg(test)]
use proptest::prelude::*;
use tokio::io::{AsyncRead, AsyncWrite};

pub const MAX_PAYLOAD: usize = libc::PIPE_BUF;

#[expect(
    clippy::transmute_ptr_to_ref,
    reason = "const_concat_bytes! emits this unfortunately"
)]
const HELLO_PAYLOAD: &[u8; MAX_PAYLOAD] = {
    const MAGIC: &[u8] = concat!("stdiomux piped v", env!("CARGO_PKG_VERSION"), "\0").as_bytes();
    const_concat_bytes!(MAGIC, &[0u8; MAX_PAYLOAD - MAGIC.len()])
};

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Clone, Copy)]
#[cfg_attr(test, derive(proptest_derive::Arbitrary))]
pub struct ChannelId(#[cfg_attr(test, proptest(strategy = "0u16..=65535"))] pub u16);

#[derive(Debug, PartialEq, Eq, Clone, Copy, EnumDiscriminants)]
#[cfg_attr(test, derive(proptest_derive::Arbitrary), proptest(params = "()"))]
#[strum_discriminants(name(FrameType))]
#[strum_discriminants(derive(FromRepr))]
#[strum_discriminants(repr(u8))]
pub enum Header {
    MuxControl(MuxControlHeader),
    ChannelControl(ChannelId, ChannelControlHeader),
    ChannelData(ChannelId, u16),
}

#[derive(Debug, PartialEq, Eq, Clone)]
#[cfg_attr(test, derive(proptest_derive::Arbitrary), proptest(params = "()"))]
pub enum Frame {
    MuxControl(MuxControlHeader),
    ChannelControl(ChannelId, ChannelControlHeader),
    ChannelData(ChannelId, ChannelDataFrame),
}

#[derive(Debug, PartialEq, Eq, Clone, Copy, EnumDiscriminants)]
#[cfg_attr(test, derive(proptest_derive::Arbitrary), proptest(params = "()"))]
#[strum_discriminants(name(MuxControlOpcode))]
#[strum_discriminants(derive(FromRepr))]
#[strum_discriminants(repr(u8))]
pub enum MuxControlHeader {
    Reset,
    Hello,
    Terminate,
    Finished,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy, EnumDiscriminants)]
#[cfg_attr(test, derive(proptest_derive::Arbitrary), proptest(params = "()"))]
#[strum_discriminants(name(ChannelControlOpcode))]
#[strum_discriminants(derive(FromRepr))]
#[strum_discriminants(repr(u8))]
pub enum ChannelControlHeader {
    Reset,
    Open(u8),
    Admit(u8),
}

#[derive(Debug, PartialEq, Eq, Clone)]
#[cfg_attr(test, derive(proptest_derive::Arbitrary), proptest(params = "()"))]
pub struct ChannelDataFrame(#[cfg_attr(test, proptest(strategy = "payload_strategy()"))] pub Bytes);

#[cfg(test)]
pub fn payload_strategy() -> impl Strategy<Value = Bytes> {
    proptest::collection::vec(proptest::num::u8::ANY, 0..=MAX_PAYLOAD).prop_map(|x| Bytes::from(x))
}

#[derive(Debug, thiserror::Error)]
pub enum SerializeFrameError {
    #[error("Frame payload is too long")]
    FrameTooLong,
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
}

impl Header {
    /// ## Panics
    ///
    /// - If payload length field in ChannelData is too big
    pub fn serialized(&self) -> u32 {
        let frame_type = self.discriminant() as u32;

        let argument = match self {
            Header::MuxControl(f) => {
                let opcode = f.discriminant() as u32;
                opcode << 24
            }
            Header::ChannelControl(id, f) => {
                let opcode = f.discriminant() as u32;
                let argument: u8 = match f {
                    ChannelControlHeader::Reset => 0,
                    ChannelControlHeader::Open(size) => *size,
                    ChannelControlHeader::Admit(permits) => *permits,
                };
                opcode << 24 | (argument as u32) << 16 | id.0 as u32
            }
            Header::ChannelData(id, f) => {
                if *f as usize > MAX_PAYLOAD {
                    panic!(
                        "Payload length of {f} exceeds maximum payload length of {MAX_PAYLOAD}!"
                    );
                }
                (*f as u32) << 16 | id.0 as u32
            }
        };
        frame_type << 28 | argument
    }

    pub fn deserialize(header: u32) -> Result<Self, DeserializeFrameError> {
        let frame_type = (header >> 28) as u8;
        let frame_type = FrameType::from_repr(frame_type)
            .ok_or(DeserializeFrameError::InvalidFrameType(frame_type))?;

        let argument = header & 0x0fffffff; // mask out top 4 bits
        Ok(match frame_type {
            FrameType::MuxControl => {
                let opcode = (argument >> 24) as u8;
                let opcode = MuxControlOpcode::from_repr(opcode)
                    .ok_or(DeserializeFrameError::InvalidMuxControlOpcode(opcode))?;
                Header::MuxControl(match opcode {
                    MuxControlOpcode::Reset => MuxControlHeader::Reset,
                    MuxControlOpcode::Hello => MuxControlHeader::Hello,
                    MuxControlOpcode::Terminate => MuxControlHeader::Terminate,
                    MuxControlOpcode::Finished => MuxControlHeader::Finished,
                })
            }
            FrameType::ChannelControl => {
                let opcode = (argument >> 24) as u8;
                let opcode: ChannelControlOpcode = ChannelControlOpcode::from_repr(opcode)
                    .ok_or(DeserializeFrameError::InvalidChannelControlOpcode(opcode))?;
                let channel_argument = (argument >> 16) as u8;
                let channel_id = ChannelId((argument & 0xffff) as u16);

                Header::ChannelControl(
                    channel_id,
                    match opcode {
                        ChannelControlOpcode::Reset => ChannelControlHeader::Reset,
                        ChannelControlOpcode::Open => ChannelControlHeader::Open(channel_argument),
                        ChannelControlOpcode::Admit => {
                            ChannelControlHeader::Admit(channel_argument)
                        }
                    },
                )
            }
            FrameType::ChannelData => {
                let length = (argument >> 16) as usize;
                let channel_id = ChannelId((argument & 0xffff) as u16);
                Header::ChannelData(channel_id, length as u16)
            }
        })
    }

    pub fn payload_length(&self) -> u16 {
        match self {
            Header::MuxControl(h) => match h {
                MuxControlHeader::Hello => HELLO_PAYLOAD.len().try_into().unwrap(),
                MuxControlHeader::Reset
                | MuxControlHeader::Terminate
                | MuxControlHeader::Finished => 0,
            },
            Header::ChannelControl(_, h) => match h {
                ChannelControlHeader::Reset
                | ChannelControlHeader::Open(_)
                | ChannelControlHeader::Admit(_) => 0,
            },
            Header::ChannelData(_, payload_len) => *payload_len,
        }
    }

    /// ## Panics
    /// - Payload is not of expected length
    pub fn with_payload(self, payload: Bytes) -> Result<Frame, DeserializeFrameError> {
        Ok(match self {
            Header::MuxControl(h) => Frame::MuxControl(match h {
                MuxControlHeader::Hello => {
                    if &payload[..] != HELLO_PAYLOAD {
                        let cstr = CStr::from_bytes_until_nul(&payload)
                            .map(|s| s.to_string_lossy())
                            .unwrap_or_else(|_| String::from_utf8_lossy(&payload));
                        Err(DeserializeFrameError::InvalidHello(cstr.to_string()))?;
                    }
                    MuxControlHeader::Hello
                }
                other @ (MuxControlHeader::Reset
                | MuxControlHeader::Terminate
                | MuxControlHeader::Finished) => other,
            }),
            Header::ChannelControl(id, h) => Frame::ChannelControl(
                id,
                match h {
                    other @ (ChannelControlHeader::Reset
                    | ChannelControlHeader::Open(_)
                    | ChannelControlHeader::Admit(_)) => other,
                },
            ),
            Header::ChannelData(id, payload_len) => {
                if payload.len() != payload_len as usize {
                    panic!(
                        "Payload length mismatch! Expected {}, got {}",
                        payload_len,
                        payload.len()
                    );
                }
                Frame::ChannelData(id, ChannelDataFrame(payload))
            }
        })
    }
}

impl Frame {
    pub fn header(&self) -> Header {
        match self {
            Frame::MuxControl(h) => Header::MuxControl(h.clone()),
            Frame::ChannelControl(id, h) => Header::ChannelControl(*id, h.clone()),
            Frame::ChannelData(id, f) => {
                let len: usize = f.0.len();
                if f.0.len() > MAX_PAYLOAD {
                    panic!(
                        "Payload length of {len} exceeds maximum payload length of {MAX_PAYLOAD}!",
                    );
                }
                Header::ChannelData(*id, len as u16)
            }
        }
    }

    pub fn payload(&self) -> Bytes {
        match self {
            Frame::MuxControl(h) => match h {
                MuxControlHeader::Hello => Bytes::from_static(HELLO_PAYLOAD.as_slice()),
                MuxControlHeader::Reset
                | MuxControlHeader::Terminate
                | MuxControlHeader::Finished => Bytes::new(),
            },
            Frame::ChannelControl(_, h) => match h {
                ChannelControlHeader::Reset
                | ChannelControlHeader::Open(_)
                | ChannelControlHeader::Admit(_) => Bytes::new(),
            },
            Frame::ChannelData(_, channel_data_frame) => channel_data_frame.0.clone(),
        }
    }
}

pub trait WriteExt: std::io::Write {
    fn write_frame(&mut self, frame: &Frame) -> std::io::Result<()> {
        let header = frame.header();
        let header_bytes = header.serialized().to_be_bytes();

        let total_packet_length = header_bytes.len() + (header.payload_length() as usize);

        if total_packet_length < libc::PIPE_BUF {
            // If packet size is less than PIPE_BUF, bunch it together into one buffer and write it
            // once to keep it atomic
            let mut buf = BytesMut::with_capacity(total_packet_length);
            buf.put_slice(&header_bytes);
            buf.put_slice(&frame.payload());
            self.write_all(&buf)?;
        } else {
            // Otherwise, split into two atomic writes -- one of just the header, and one of the
            // payload
            self.write_all(&header_bytes)?;
            self.write_all(&frame.payload())?;
        }
        Ok(())
    }
}
impl<W: std::io::Write> WriteExt for W {}

pub trait AsyncWriteExt: AsyncWrite {
    fn write_frame_async<'a>(
        &'a mut self,
        frame: &'a Frame,
    ) -> impl std::future::Future<Output = std::io::Result<()>> + 'a
    where
        Self: Unpin,
    {
        async {
            let header = frame.header();
            let header_bytes = header.serialized().to_be_bytes();

            let total_packet_length = header_bytes.len() + (header.payload_length() as usize);

            if total_packet_length < libc::PIPE_BUF {
                // If packet size is less than PIPE_BUF, bunch it together into one buffer and write it
                // once to keep it atomic
                let mut buf = BytesMut::with_capacity(total_packet_length);
                buf.put_slice(&header_bytes);
                buf.put_slice(&frame.payload());
                tokio::io::AsyncWriteExt::write_all(self, &buf).await?;
            } else {
                // Otherwise, split into two atomic writes -- one of just the header, and one of the
                // payload
                tokio::io::AsyncWriteExt::write_all(self, &header_bytes).await?;
                tokio::io::AsyncWriteExt::write_all(self, &frame.payload()).await?;
            }
            Ok(())
        }
    }
}

impl<W: AsyncWrite> AsyncWriteExt for W {}

pub trait ReadExt: std::io::Read {
    fn read_frame(&mut self) -> std::io::Result<Frame> {
        let mut header = [0u8; 4];
        self.read_exact(&mut header)?;
        let header = Header::deserialize(u32::from_be_bytes(header))
            .map_err(|e| std::io::Error::new(ErrorKind::InvalidData, e))?;

        let mut buf = BytesMut::zeroed(header.payload_length() as usize);
        self.read_exact(&mut buf)?;

        let f = header
            .with_payload(buf.freeze())
            .map_err(|e| std::io::Error::new(ErrorKind::InvalidData, e))?;
        Ok(f)
    }
}

impl<R: std::io::Read> ReadExt for R {}

pub trait AsyncReadExt: AsyncRead {
    fn read_frame_async<'a>(
        &'a mut self,
    ) -> impl std::future::Future<Output = std::io::Result<Frame>> + 'a
    where
        Self: Unpin,
    {
        async {
            let mut header = [0u8; 4];
            tokio::io::AsyncReadExt::read_exact(self, &mut header).await?;
            let header = Header::deserialize(u32::from_be_bytes(header))
                .map_err(|e| std::io::Error::new(ErrorKind::InvalidData, e))?;

            let mut buf = BytesMut::zeroed(header.payload_length() as usize);
            tokio::io::AsyncReadExt::read_exact(self, &mut buf).await?;

            let f = header
                .with_payload(buf.freeze())
                .map_err(|e| std::io::Error::new(ErrorKind::InvalidData, e))?;
            Ok(f)
        }
    }
}

impl<R: AsyncRead> AsyncReadExt for R {}

#[cfg(test)]
mod tests {
    use test_strategy::proptest;

    use super::*;

    #[proptest]
    fn test_serialize_roundtrip(frame: Frame) {
        let mut buf: Vec<u8> = vec![];
        buf.write_frame(&frame).unwrap();
        println!(
            "serialized header: {:032b}",
            u32::from_be_bytes([buf[0], buf[1], buf[2], buf[3]])
        );

        let mut cursor = &buf[..];
        let result = cursor.read_frame().unwrap();

        assert_eq!(result, frame, "not the same frame");
        assert!(cursor.is_empty(), "did not consume all produced data");
    }

    #[proptest(async = "tokio")]
    async fn test_serialize_roundtrip_async(frame: Frame) {
        let mut buf: Vec<u8> = vec![];
        buf.write_frame_async(&frame).await.unwrap();
        println!(
            "serialized header: {:032b}",
            u32::from_be_bytes([buf[0], buf[1], buf[2], buf[3]])
        );

        let mut cursor = &buf[..];
        let result = cursor.read_frame_async().await.unwrap();

        assert_eq!(result, frame, "not the same frame");
        assert!(cursor.is_empty(), "did not consume all produced data");
    }
}
