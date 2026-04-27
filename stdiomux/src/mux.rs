use std::sync::Arc;

use bytes::{Bytes, BytesMut};
use futures::{Sink, stream::Stream};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader, BufWriter};

pub const VERSION_1_MAGIC_HANDSHAKE: &[u8; 16] = b"stdiomux\0\0\0\0\0\0\0\x01";
pub const MTU: usize = 65536;
pub const HEADER_BYTES: usize = 6;
pub const MAX_BODY: usize = MTU - HEADER_BYTES;

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Clone, Copy)]
pub struct StreamId(pub u16);

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Clone, Copy, strum::FromRepr)]
#[repr(u8)]
pub enum FrameType {
    RST = 0,
    DAT = 1,
    ADM = 2,
    SYN = 3,
    FIN = 4,
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Clone)]
pub enum Frame {
    Reset,
    Data(bytes::Bytes),
    Adm(u64),
    Syn,
    Fin,
}

impl Frame {
    pub fn type_field(&self) -> FrameType {
        match self {
            Frame::Reset => FrameType::RST,
            Frame::Data(_) => FrameType::DAT,
            Frame::Adm(_) => FrameType::ADM,
            Frame::Syn => FrameType::SYN,
            Frame::Fin => FrameType::FIN,
        }
    }

    pub fn body(&self) -> Bytes {
        match self {
            Frame::Reset => Bytes::new(),
            Frame::Data(bytes) => bytes.clone(),
            Frame::Adm(seqno) => Bytes::copy_from_slice(&seqno.to_be_bytes()),
            Frame::Syn => Bytes::new(),
            Frame::Fin => Bytes::new(),
        }
    }
}

impl From<Frame> for FrameType {
    fn from(f: Frame) -> Self {
        f.type_field()
    }
}

pub async fn initialize_mux<W: AsyncWrite + Unpin, R: AsyncRead + Unpin>(
    mut w: W,
    mut r: R,
) -> std::io::Result<(MuxWriter<W>, MuxReader<R>)> {
    // send symmetric sanity check handshake
    w.write(VERSION_1_MAGIC_HANDSHAKE).await?;
    w.flush().await?;

    // recv and validate handshake
    let mut buf = vec![0u8; VERSION_1_MAGIC_HANDSHAKE.len()];
    r.read_exact(&mut buf).await?;
    if buf != VERSION_1_MAGIC_HANDSHAKE {
        return Err(std::io::Error::new(
            std::io::ErrorKind::ConnectionAborted,
            "bad",
        ));
    }

    let w = MuxWriter::new(w);
    let r = MuxReader::new(r);

    Ok((w, r))
}

/// Very low-level raw frame writing utility.
pub struct MuxWriter<W: AsyncWrite + Unpin> {
    w: Arc<tokio::sync::Mutex<BufWriter<W>>>,
}

// manual trait impl needed
impl<W: AsyncWrite + Unpin> Clone for MuxWriter<W> {
    fn clone(&self) -> Self {
        Self { w: self.w.clone() }
    }
}

impl<W: AsyncWrite + Unpin> MuxWriter<W> {
    /// Construct a [MuxWriter]. WARNING: This does not send the initial handshake!
    ///
    /// WARNING: This does not send the initial handshake! To do that, use [initialize_mux]!
    pub fn new(w: W) -> Self {
        Self {
            w: Arc::new(tokio::sync::Mutex::new(BufWriter::with_capacity(MTU, w))),
        }
    }

    pub async fn sendto(&self, stream_id: StreamId, frame: &Frame) -> std::io::Result<()> {
        let body = frame.body();
        if body.len() > MAX_BODY {
            return Err(std::io::Error::new(
                std::io::ErrorKind::FileTooLarge,
                format!(
                    "error while writing mux frame: body length of {} exceeds maximum of {MAX_BODY}",
                    body.len()
                ),
            ));
        }

        let mut w = self.w.lock().await;
        w.write_u16(body.len() as u16).await?; // body len
        w.write_u16(stream_id.0).await?; // stream id
        w.write_u8(frame.type_field() as u8).await?; // type
        w.write_u8(0).await?; // reserved
        w.write_all(&body).await?; // body
        w.flush().await?;

        Ok(())
    }

    pub fn as_sink(&self) -> impl Sink<(StreamId, Frame), Error = std::io::Error> {
        futures::sink::unfold(self.clone(), |this, (stream_id, frame)| async move {
            this.sendto(stream_id, &frame).await?;
            Ok(this)
        })
    }
}

/// Very low-level raw frame reading utility.
pub struct MuxReader<R: AsyncRead + Unpin> {
    r: BufReader<R>,
}

impl<R: AsyncRead + Unpin> MuxReader<R> {
    /// Construct a [MuxReader] that has been initialized.
    ///
    /// WARNING: This does not validate the initial handshake! To do that, use [initialize_mux]!
    pub fn new(r: R) -> Self {
        Self {
            r: BufReader::with_capacity(MTU, r),
        }
    }

    pub async fn recvfrom(&mut self) -> std::io::Result<(StreamId, Frame)> {
        // Read header
        let body_len = self.r.read_u16().await? as usize;
        let stream_id = StreamId(self.r.read_u16().await?);
        let frame_type = self.r.read_u8().await?;
        let _reserved = self.r.read_u8().await?;

        // Validate header
        if body_len > MAX_BODY {
            return Err(std::io::Error::new(
                std::io::ErrorKind::ConnectionAborted,
                format!(
                    "aborted while reading mux frame: body length of {body_len} exceeds maximum of {MAX_BODY}",
                ),
            ));
        }
        let Some(frame_type) = FrameType::from_repr(frame_type) else {
            return Err(std::io::Error::new(
                std::io::ErrorKind::ConnectionAborted,
                format!("aborted while reading mux frame: unrecognized frame type {frame_type}"),
            ));
        };

        // Read body
        let mut buf = BytesMut::zeroed(body_len);
        self.r.read_exact(&mut buf).await?;

        let frame = match frame_type {
            FrameType::RST => Frame::Reset,
            FrameType::DAT => Frame::Data(buf.freeze()),
            FrameType::SYN => Frame::Syn,
            FrameType::ADM => {
                if body_len < 8 {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::ConnectionAborted,
                        format!("aborted while reading ADM packet: invalid body format"),
                    ));
                }
                let mut bytes = [0u8; 8];
                bytes.copy_from_slice(&buf[0..8]);
                Frame::Adm(u64::from_be_bytes(bytes))
            }
            FrameType::FIN => todo!(),
        };

        Ok((stream_id, frame))
    }

    pub fn as_stream<'a>(
        &'a mut self,
    ) -> impl Stream<Item = std::io::Result<(StreamId, Frame)>> + 'a {
        futures::stream::unfold(
            self,
            |this| async move { Some((this.recvfrom().await, this)) },
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::*;

    #[rstest]
    #[tokio::test]
    async fn round_trip(
        #[values(0, 1, 100, 65535)] stream_id: u16,
        #[values(0, 1, 100, MAX_BODY)] len: usize,
    ) {
        let stream_id = StreamId(stream_id);
        let payload = Frame::Data(Bytes::from(vec![0u8; len]));

        let mut ser = vec![];
        MuxWriter::new(&mut ser)
            .sendto(stream_id, &payload)
            .await
            .unwrap();
        let result = MuxReader::new(ser.as_slice()).recvfrom().await.unwrap();

        assert_eq!(result, (stream_id, payload))
    }

    #[rstest]
    #[tokio::test]
    async fn payload_too_big(
        #[values(MAX_BODY+1, MAX_BODY+2, MAX_BODY+10, MAX_BODY*2)] len: usize,
    ) {
        let stream_id = StreamId(0);
        let payload = Frame::Data(Bytes::from(vec![0u8; len]));

        let mut ser = vec![];

        MuxWriter::new(&mut ser)
            .sendto(stream_id, &payload)
            .await
            .expect_err("Did not error when sending excessively large payload");
    }
}
