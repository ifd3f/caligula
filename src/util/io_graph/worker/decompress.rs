use std::{io::Read, marker::PhantomData};

use crate::{
    codec::compression::{CompressionFormat, decompress},
    util::{
        alloc_uninit_bytes_with_layout,
        io_graph::{
            ALLOC_LAYOUT, GraphContext, RecvBytes, SendBytes, Worker, util::RecvBytesReader,
        },
    },
};

#[derive(Debug, thiserror::Error)]
#[error("{error}")]
pub struct DecompressError {
    // TODO get rid of this anyhow usage
    #[from]
    error: anyhow::Error,
}

pub fn decompressor(
    cf: CompressionFormat,
    rx: impl RecvBytes,
    mut tx: impl SendBytes,
) -> impl Worker<Output = (), Error = DecompressError> {
    move |_ctx: &GraphContext| {
        // Strategy is to read this decompressor into an intermediate BytesMut,
        // then send off the bytes chunk-by-chunk.
        let mut read = decompress(cf, RecvBytesReader::from(rx))?;
        let mut reader_empty = false;

        while !reader_empty {
            // SAFETY: these bytes will get filled up immediately. everything
            // else that wasn't filled up will get truncated
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
