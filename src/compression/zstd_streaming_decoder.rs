//! This code is copied from [zstd-rs PR #62](https://github.com/KillingSpark/zstd-rs/pull/62).
//! It is vendored in and slightly modified because I am too impatient to wait for it
//! to be merged and published to crates.io, and nixpkgs does not enjoy git dependencies.

#![allow(dead_code)]

// MIT License
//
// Copyright (c) 2019 Moritz Borcherding
//
// Permission is hereby granted, free of charge, to any person obtaining a copy
// of this software and associated documentation files (the "Software"), to deal
// in the Software without restriction, including without limitation the rights
// to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
// copies of the Software, and to permit persons to whom the Software is
// furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in all
// copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
// IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
// OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
// SOFTWARE.

use core::borrow::BorrowMut;

use ruzstd::frame_decoder::{BlockDecodingStrategy, FrameDecoder, FrameDecoderError};
use ruzstd::io::{Error, ErrorKind, Read};

/// High level decoder that implements a io::Read that can be used with
/// io::Read::read_to_end / io::Read::read_exact or passing this to another library / module as a source for the decoded content
///
/// The lower level FrameDecoder by comparison allows for finer grained control but need sto have it's decode_blocks method called continuously
/// to decode the zstd-frame.
///
/// ## Caveat
/// [StreamingDecoder] expects the underlying stream to only contain a single frame.
/// To decode all the frames in a finite stream, the calling code needs to recreate
/// the instance of the decoder
/// and handle
/// [crate::frame::ReadFrameHeaderError::SkipFrame]
/// errors by skipping forward the `length` amount of bytes, see <https://github.com/KillingSpark/zstd-rs/issues/57>
pub struct StreamingDecoder<READ: Read, DEC: BorrowMut<FrameDecoder>> {
    pub decoder: DEC,
    source: READ,
}

impl<READ: Read, DEC: BorrowMut<FrameDecoder>> StreamingDecoder<READ, DEC> {
    pub fn new_with_decoder(
        mut source: READ,
        mut decoder: DEC,
    ) -> Result<StreamingDecoder<READ, DEC>, FrameDecoderError> {
        decoder.borrow_mut().init(&mut source)?;
        Ok(StreamingDecoder { decoder, source })
    }
}

impl<READ: Read> StreamingDecoder<READ, FrameDecoder> {
    pub fn new(
        mut source: READ,
    ) -> Result<StreamingDecoder<READ, FrameDecoder>, FrameDecoderError> {
        let mut decoder = FrameDecoder::new();
        decoder.init(&mut source)?;
        Ok(StreamingDecoder { decoder, source })
    }
}

impl<READ: Read, DEC: BorrowMut<FrameDecoder>> StreamingDecoder<READ, DEC> {
    /// Gets a reference to the underlying reader.
    pub fn get_ref(&self) -> &READ {
        &self.source
    }

    /// Gets a mutable reference to the underlying reader.
    ///
    /// It is inadvisable to directly read from the underlying reader.
    pub fn get_mut(&mut self) -> &mut READ {
        &mut self.source
    }

    /// Destructures this object into the inner reader.
    pub fn into_inner(self) -> READ
    where
        READ: Sized,
    {
        self.source
    }

    /// Destructures this object into both the inner reader and [FrameDecoder].
    pub fn into_parts(self) -> (READ, DEC)
    where
        READ: Sized,
    {
        (self.source, self.decoder)
    }

    /// Destructures this object into the inner [FrameDecoder].
    pub fn into_frame_decoder(self) -> DEC {
        self.decoder
    }
}

impl<READ: Read, DEC: BorrowMut<FrameDecoder>> Read for StreamingDecoder<READ, DEC> {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, Error> {
        let decoder = self.decoder.borrow_mut();
        if decoder.is_finished() && decoder.can_collect() == 0 {
            //No more bytes can ever be decoded
            return Ok(0);
        }

        // need to loop. The UpToBytes strategy doesn't take any effort to actually reach that limit.
        // The first few calls can result in just filling the decode buffer but these bytes can not be collected.
        // So we need to call this until we can actually collect enough bytes

        // TODO add BlockDecodingStrategy::UntilCollectable(usize) that pushes this logic into the decode_blocks function
        while decoder.can_collect() < buf.len() && !decoder.is_finished() {
            //More bytes can be decoded
            let additional_bytes_needed = buf.len() - decoder.can_collect();
            match decoder.decode_blocks(
                &mut self.source,
                BlockDecodingStrategy::UptoBytes(additional_bytes_needed),
            ) {
                Ok(_) => { /*Nothing to do*/ }
                Err(e) => {
                    let err;
                    #[cfg(feature = "std")]
                    {
                        err = Error::new(ErrorKind::Other, e);
                    }
                    #[cfg(not(feature = "std"))]
                    {
                        err = Error::new(ErrorKind::Other, Box::new(e));
                    }
                    return Err(err);
                }
            }
        }

        decoder.read(buf)
    }
}
