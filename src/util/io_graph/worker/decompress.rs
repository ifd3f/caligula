use std::{io::Read, marker::PhantomData};

use crate::{
    codec::compression::{CompressionFormat, decompress},
    util::{
        alloc_uninit_bytes_with_layout,
        io_graph::{
            ALLOC_LAYOUT, OldGraphContext, RecvBytes, SendBytes, Worker, util::RecvBytesReader,
        },
    },
};

pub struct DecompressorWorker {
    cf: CompressionFormat,
    _phantom: PhantomData<()>,
}

#[derive(Debug, thiserror::Error)]
#[error("{error}")]
pub struct DecompressError {
    // TODO get rid of this
    #[from]
    error: anyhow::Error,
}

impl DecompressorWorker {
    pub fn new(cf: CompressionFormat) -> Box<Self> {
        Box::new(Self {
            cf,
            _phantom: PhantomData,
        })
    }
}

impl<Rx: RecvBytes, Tx: SendBytes> Worker<(Rx, Tx)> for DecompressorWorker {
    type Error = DecompressError;
    type Output = ();

    fn run(
        self: Box<Self>,
        _context: &OldGraphContext,
        (rx, mut tx): (Rx, Tx),
    ) -> Result<Self::Output, Self::Error> {
        // Strategy is to read this decompressor into an intermediate BytesMut, then
        // send off the bytes chunk-by-chunk.
        let mut read = decompress(self.cf, RecvBytesReader::from(rx))?;
        let mut reader_empty = false;

        while !reader_empty {
            // SAFETY: these bytes will get filled up immediately. everything else that
            // wasn't filled up will get truncated
            let mut buf = unsafe { alloc_uninit_bytes_with_layout(ALLOC_LAYOUT) };

            // fill up the buffer as much as possible
            let mut cursor = buf.as_mut();
            while !reader_empty && !cursor.is_empty() {
                let count = read.read(cursor).map_err(anyhow::Error::from)?;

                if count == 0 {
                    // 0 bytes means EOF
                    reader_empty = true;
                    break;
                } else {
                    // advance cursor past the bytes we just filled
                    cursor = &mut cursor[count..];
                }
            }

            // calculate how many bytes we filled and ship it off
            let unfilled_bytes = cursor.len();
            buf.truncate(buf.len() - unfilled_bytes);
            tx.send(buf.freeze()).map_err(anyhow::Error::from)?;
        }

        tx.close().map_err(anyhow::Error::from)?;

        Ok(())
    }
}
