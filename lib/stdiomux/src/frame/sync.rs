//! [`std::io`]-based [Frame] serialization and deserialization utilities.

use std::{
    io::{Read, Write},
    marker::PhantomData,
};

use bytes::{Bytes, BytesMut};

use super::{Frame, Header, ReadFrameError, WriteFrameError};

/// [`std::io::Write`]-based frame serializer.
pub struct FrameWriter<W: Write, F: Frame> {
    w: W,
    _phantom: PhantomData<F>,
}

impl<W: Write, F: Frame> FrameWriter<W, F> {
    pub fn new(w: W) -> Self {
        Self {
            w,
            _phantom: PhantomData,
        }
    }

    /// Write the provided frame to the underlying [Write].
    pub fn write_frame(&mut self, f: F) -> Result<(), WriteFrameError<F>> {
        let len = F::Header::SIZE + f.header().body_len();
        let mut buf = vec![0u8; len];

        f.serialize(&mut buf).map_err(WriteFrameError::Frame)?;

        self.w.write_all(&buf)?;
        Ok(())
    }
}

/// [`std::io::Read`]-based frame deserializer.
pub struct FrameReader<R: Read, F: Frame> {
    r: R,
    _phantom: PhantomData<F>,
}

impl<R: Read, F: Frame> FrameReader<R, F> {
    const HEADER_SIZE: usize = <F::Header as Header>::SIZE;

    pub fn new(r: R) -> Self {
        Self {
            r,
            _phantom: PhantomData,
        }
    }

    /// Read a single frame off the underlying [Read].
    pub fn read_frame(&mut self) -> Result<F, ReadFrameError<F>> {
        // create uninitialized MTU-length BytesMut
        let mut buf = BytesMut::with_capacity(F::MTU);

        // safety for all unsafe blocks:
        // setting the length is safe because we are filling these bytes before they get read.
        // yes, technically R's impl can read the uninitialized data for whatever,
        // but in practice, if you're worried about that, that's kinda your problem lol
        unsafe {
            buf.set_len(F::MTU);
        }

        // split it into header and body part
        let mut body = buf.split_off(Self::HEADER_SIZE);
        let mut header = buf;

        // read and deserialize header
        self.r.read_exact(&mut header)?;
        let header =
            <F::Header as Header>::deserialize(header.freeze()).map_err(ReadFrameError::Header)?;

        match header.body_len() {
            0 => {
                // body is zero. no need to read anything.
                Ok(F::deserialize(header, Bytes::new()).map_err(ReadFrameError::Body)?)
            }
            len => {
                // read and deserialize body
                body.truncate(len);
                self.r.read_exact(&mut body)?;
                Ok(F::deserialize(header, body.freeze()).map_err(ReadFrameError::Body)?)
            }
        }
    }
}
