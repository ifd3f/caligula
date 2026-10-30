//! Module containing common reusable workers.

use std::{
    fs::File,
    io::{Read, Write},
    path::Path,
};

use bytes::Bytes;
use digest::Digest;

use crate::{
    codec::compression::{CompressionFormat, decompress},
    util::{
        alloc_uninit_bytes_with_layout,
        io_graph::{
            ALLOC_LAYOUT, GraphContext, RecvBytes, SendBytes, Worker,
            sys::{DiskMode, open_blockdev},
            util::RecvBytesReader,
            worker_fn,
        },
    },
};

/// A worker optimized for writing to a file or disk.
///
/// It uses boring `write()` and `pwrite()` calls.
pub fn file_writer(
    path: &Path,
    mut rx: impl RecvBytes + 'static,
) -> Result<
    (
        std::fs::Metadata,
        Box<dyn Worker<Output = (), Error = std::io::Error>>,
    ),
    std::io::Error,
> {
    let mut file = open_blockdev(path, DiskMode::Write)?;
    /*
    nix::fcntl::posix_fadvise(&file, 0, 0, PosixFadviseAdvice::POSIX_FADV_SEQUENTIAL)
        .ok_or_log();
    */

    let metadata = file.metadata()?;

    let w = worker_fn(move |ctx: &GraphContext| {
        // TODO: multithread this mf
        while !ctx.halt()
            && let Some(buf) = rx.recv()?
        {
            // Even for disks, this is fine if the input arrives in exact
            // chunks.
            file.write(&buf)?;
        }

        Ok(())
    });

    Ok((metadata, w))
}

/// A worker optimized for reading a file on disk.
///
/// It uses boring `read()` calls.
pub fn file_reader(
    path: &Path,
    mut tx: impl SendBytes + 'static,
) -> Result<
    (
        std::fs::Metadata,
        Box<dyn Worker<Output = (), Error = std::io::Error>>,
    ),
    std::io::Error,
> {
    let mut file = File::open(path)?;
    let metadata = file.metadata()?;

    /*
    nix::fcntl::posix_fadvise(&file, 0, 0, PosixFadviseAdvice::POSIX_FADV_SEQUENTIAL)
        .ok_or_log();
    */

    let w = worker_fn(move |ctx: &GraphContext| {
        while !ctx.halt() {
            // SAFETY: these bytes will get filled up immediately. everything
            // else that wasn't filled up will get truncated
            let mut buf = unsafe { alloc_uninit_bytes_with_layout(ALLOC_LAYOUT) };

            let count = file.read(&mut buf)?;
            if count == 0 {
                break;
            }

            buf.truncate(count);
            tx.send(buf.freeze())?;
        }

        tx.close()?;

        Ok(())
    });

    Ok((metadata, w))
}

/// A worker that performs a hash calculation.
pub fn hasher<H: Digest>(
    mut rx: impl RecvBytes + 'static,
) -> Box<dyn Worker<Output = Bytes, Error = std::io::Error>> {
    worker_fn(move |ctx: &GraphContext| {
        let mut h = H::new();

        while !ctx.halt() {
            match rx.recv()? {
                Some(b) => h.update(b),
                None => break,
            }
        }

        let h = h.finalize();
        Ok(Bytes::copy_from_slice(&h))
    })
}

#[derive(Debug, thiserror::Error)]
#[error("{error}")]
pub struct DecompressError {
    // TODO get rid of this anyhow usage
    #[from]
    error: anyhow::Error,
}

pub fn decompressor(
    cf: CompressionFormat,
    rx: impl RecvBytes + 'static,
    mut tx: impl SendBytes + 'static,
) -> Box<dyn Worker<Output = (), Error = DecompressError>> {
    worker_fn(move |_ctx: &GraphContext| {
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
    })
}
