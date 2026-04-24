use std::{
    collections::{HashMap, hash_map::Entry},
    convert::Infallible,
    ops::ControlFlow,
    sync::Arc,
};

use bytes::{Bytes, BytesMut};
use futures::{Sink, SinkExt, stream::Stream};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

pub const VERSION_1_MAGIC_HANDSHAKE: &[u8] = b"ipcmux\0\0\0\0\0\x01";
pub const MTU: usize = 65536;
pub const HEADER_LEN: usize = 4;
pub const MAX_PAYLOAD: usize = MTU - HEADER_LEN;

#[derive(
    Debug,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Serialize,
    Deserialize,
    Clone,
    Copy,
    derive_more::From,
    derive_more::Into,
)]
pub struct StreamId(pub u16);

#[derive(Debug, thiserror::Error)]
pub enum MuxError {
    #[error("Mismatched protocol version!")]
    ProtocolMismatch,
    #[error("Transport error: {0}")]
    Transport(#[from] std::io::Error),
    #[error("Payload too large: {0} exceeds {MAX_PAYLOAD}")]
    PayloadTooLarge(usize),
}

pub async fn initialize_mux<W: AsyncWrite + Unpin, R: AsyncRead + Unpin>(
    mut w: W,
    mut r: R,
) -> Result<(MuxWriter<W>, MuxReader<R>), MuxError> {
    // send symmetric sanity check handshake
    w.write(VERSION_1_MAGIC_HANDSHAKE).await?;
    w.flush().await?;

    // recv and validate handshake
    let mut buf = vec![0u8; VERSION_1_MAGIC_HANDSHAKE.len()];
    r.read_exact(&mut buf).await?;
    if buf != VERSION_1_MAGIC_HANDSHAKE {
        return Err(MuxError::ProtocolMismatch);
    }

    let w = MuxWriter {
        w: Arc::new(tokio::sync::Mutex::new(w)),
    };
    let r = MuxReader { r };

    Ok((w, r))
}

pub struct MuxWriter<W: AsyncWrite + Unpin> {
    w: Arc<tokio::sync::Mutex<W>>,
}

// manual trait impl needed
impl<W: AsyncWrite + Unpin> Clone for MuxWriter<W> {
    fn clone(&self) -> Self {
        Self { w: self.w.clone() }
    }
}

impl<W: AsyncWrite + Unpin> From<W> for MuxWriter<W> {
    fn from(w: W) -> Self {
        Self::new(w)
    }
}

impl<W: AsyncWrite + Unpin> MuxWriter<W> {
    pub fn new(w: W) -> Self {
        Self {
            w: Arc::new(tokio::sync::Mutex::new(w)),
        }
    }

    pub async fn send(&self, stream_id: StreamId, buf: impl AsRef<[u8]>) -> Result<(), MuxError> {
        let buf = buf.as_ref();
        if buf.len() > MAX_PAYLOAD {
            return Err(MuxError::PayloadTooLarge(buf.len()));
        }

        let header = ((stream_id.0 as u32) << 16) | (buf.len() as u32);

        let mut w = self.w.lock().await;
        w.write_u32(header).await?;
        w.write_all(buf).await?;

        Ok(())
    }

    pub fn as_sink(&self) -> impl Sink<(StreamId, Bytes), Error = MuxError> {
        futures::sink::unfold(self.clone(), |this, (stream_id, datagram)| async move {
            this.send(stream_id, datagram).await?;
            Ok(this)
        })
    }

    pub async fn as_stream_sink(&self, stream_id: StreamId) -> impl Sink<Bytes, Error = MuxError> {
        self.as_sink()
            .with(move |x| std::future::ready(Ok((stream_id, x))))
    }
}

pub struct MuxReader<R: AsyncRead + Unpin> {
    r: R,
}

impl<R: AsyncRead + Unpin> MuxReader<R> {
    pub fn new(r: R) -> Self {
        Self { r }
    }

    pub async fn recv(&mut self) -> std::io::Result<(StreamId, Bytes)> {
        // Decode header
        let header = self.r.read_u32().await?;
        let stream_id = StreamId((header >> 16) as u16);
        let payload_size = (header & 0xffff) as usize;

        let mut buf = BytesMut::zeroed(payload_size);
        self.r.read_exact(&mut buf).await?;
        Ok((stream_id, buf.freeze()))
    }

    pub fn as_stream<'a>(
        &'a mut self,
    ) -> impl Stream<Item = std::io::Result<(StreamId, Bytes)>> + 'a {
        futures::stream::unfold(self, |this| async move { Some((this.recv().await, this)) })
    }
}

impl<R: AsyncRead + Unpin> From<R> for MuxReader<R> {
    fn from(r: R) -> Self {
        Self::new(r)
    }
}

#[derive(Clone)]
pub struct Demux {
    inner: Arc<std::sync::Mutex<HashMap<StreamId, Box<dyn DatagramHandler>>>>,
}

#[derive(Clone)]
pub struct DemuxController {
    inner: Arc<std::sync::Mutex<HashMap<StreamId, Box<dyn DatagramHandler>>>>,
}

impl DemuxController {
    pub async fn set_stream_callback(&mut self, stream_id: StreamId, h: Box<dyn DatagramHandler>) {
        self.inner.lock().unwrap().insert(stream_id, h);
    }
}

impl Demux {
    pub fn new() -> (Self, DemuxController) {
        let inner = Arc::new(std::sync::Mutex::new(HashMap::new()));
        (
            Self {
                inner: Default::default(),
            },
            DemuxController { inner },
        )
    }

    pub async fn as_sink(&self) -> impl Sink<(StreamId, Result<Bytes, Arc<MuxError>>)> {
        futures::sink::unfold(self.clone(), |this, (stream_id, data)| {
            this.handle_datagram(stream_id, data);
            std::future::ready(Ok::<Self, Infallible>(this))
        })
    }

    pub fn handle_datagram(&self, stream_id: StreamId, data: Result<Bytes, Arc<MuxError>>) {
        match self.inner.lock().unwrap().entry(stream_id) {
            // there exists a callback
            Entry::Occupied(mut occupied_entry) => {
                match occupied_entry.get_mut().handle_datagram(data) {
                    // callback wants to continue
                    ControlFlow::Continue(_) => {}

                    // callback wants to be removed
                    ControlFlow::Break(_) => {
                        occupied_entry.remove();
                    }
                }
            }

            // there is no callback -- void the data
            Entry::Vacant(_) => {}
        }
    }
}

/// Trait for generic callbacks that handle datagrams.
pub trait DatagramHandler {
    /// Handle the provided datagram or error. Returns [`ControlFlow::Continue`] to signal
    /// continuation, and [`ControlFlow::Break`] to be removed from the stream handler.
    fn handle_datagram(&self, res: Result<Bytes, Arc<MuxError>>) -> ControlFlow<()>;
}

impl<F> DatagramHandler for F
where
    F: Fn(Result<Bytes, Arc<MuxError>>) -> ControlFlow<()>,
{
    fn handle_datagram(&self, res: Result<Bytes, Arc<MuxError>>) -> ControlFlow<()> {
        self(res)
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
        #[values(0, 1, 100, 65532)] len: usize,
    ) {
        let stream_id = StreamId(stream_id);
        let payload = Bytes::from(vec![0u8; len]);

        let mut ser = vec![];
        MuxWriter::new(&mut ser)
            .send(stream_id, &payload)
            .await
            .unwrap();
        let result = MuxReader::new(ser.as_slice()).recv().await.unwrap();

        assert_eq!(result, (stream_id, payload))
    }

    #[rstest]
    #[tokio::test]
    async fn payload_too_big(#[values(65533, 65536, 108321)] len: usize) {
        let stream_id = StreamId(0);
        let payload = Bytes::from(vec![0u8; len]);

        let mut ser = vec![];

        MuxWriter::new(&mut ser)
            .send(stream_id, &payload)
            .await
            .expect_err("Did not error when sending excessively large payload");
    }
}
