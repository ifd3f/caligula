use std::{
    cmp::min,
    io::{BufRead, Read, Write},
    pin::pin,
    sync::{
        Arc,
        atomic::{self, AtomicU64},
    },
};

use bytes::{BufMut, Bytes, BytesMut};
use futures::{future::join_all, stream::FuturesUnordered};
use itertools::Itertools;
use pin_project::pin_project;
use tokio::{
    io::{AsyncBufRead, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader},
    sync::{broadcast, mpsc},
};

use crate::{
    compression::{decompress, CompressionFormat},
    herding::writer::factory::WriterState,
    util::{AsyncCountRead, ByteChannel},
};

pub fn setup_pipeline(
    input: impl AsyncRead + Send + Sync + Unpin + 'static,
    cf: CompressionFormat,
    state: Arc<WriterState>,
    buf_size: usize,
    mut outputs: Vec<impl AsyncWrite>,
) {
    const PIPE_SIZE: usize = 1 << 8;
    const READBUF_SIZE: usize = 1 << 16;

    let (dz_rx, reader_tx) = mpsc::channel(PIPE_SIZE);
    let (writer_rx, dz_tx) = broadcast::channel(PIPE_SIZE);
    let mut tee_txs = vec![];
    let mut writer_rxs = vec![];
    for o in &outputs {
        let (rx, tx) = tokio::io::simplex(PIPE_SIZE);
        writer_rxs.push(rx);
        tee_txs.push(tx);
    }

    tokio::spawn(reader(
        input,
        state.raw_src_bytes_read.clone(),
        READBUF_SIZE,
        reader_tx,
    ));

    tokio::task::spawn_blocking(|| {
        decompressor(
            std::io::BufReader::new(dz_rx),
            cf,
            state.decompressed_bytes_read.clone(),
            READBUF_SIZE,
            dz_tx,
        )
    });
}

async fn reader(
    input: impl AsyncRead + Unpin,
    raw_src_bytes_read: Arc<AtomicU64>,
    buf_size: usize,
    output: mpsc::Sender<Bytes>,
) -> std::io::Result<()> {
    let mut input = BufReader::new(AsyncCountRead::new(input, raw_src_bytes_read));

    loop {
        let mut buf = unsafe { uninit_bytesmut(buf_size) };
        let read_bytes = input.read(&mut buf).await?;
        if read_bytes == 0 {
            break;
        }
        buf.truncate(read_bytes);

        let Ok(()) = output.send(buf.freeze()).await else {
            break;
        };
    }

    Ok(())
}

fn decompressor(
    r: impl BufRead,
    cf: CompressionFormat,
    decompressed_bytes_read: Arc<AtomicU64>,
    buf_size: usize,
    output: Vec<mpsc::Sender<Bytes>>,
) -> anyhow::Result<()> {
    let mut dz = decompress(cf, r)?;

    loop {
        let mut buf = unsafe { uninit_bytesmut(buf_size) };

        let read_bytes = dz.read(&mut buf)?;

        if read_bytes == 0 {
            break;
        }
        buf.truncate(read_bytes);

        decompressed_bytes_read.fetch_add(read_bytes as u64, atomic::Ordering::Relaxed);

        output.send(buf.freeze())?;
    }

    Ok(())
}

async fn block_writer(
    mut input: impl AsyncRead + Unpin,
    decompressed_bytes_read: Arc<AtomicU64>,
    block_size: usize,
    mut output: impl AsyncWrite + Unpin,
) -> anyhow::Result<()> {
    let mut buf = vec![0u8; block_size];

    loop {
        let read_bytes = try_read_exact(&mut input, &mut buf).await?;

        if read_bytes == 0 {
            break;
        }

        decompressed_bytes_read.fetch_add(read_bytes as u64, atomic::Ordering::Relaxed);

        output.write_all(&buf[..read_bytes]).await?;
    }

    Ok(())
}

unsafe fn uninit_bytesmut(size: usize) -> BytesMut {
    let mut buf = BytesMut::with_capacity(size);
    unsafe { buf.advance_mut(size) };
    buf
}

/// Like [`ReadExt::read_exact`], but if it can't fill the entire buffer, it does not error.
#[inline(always)]
async fn try_read_exact(
    mut r: impl AsyncRead + Unpin,
    mut buf: &mut [u8],
) -> std::io::Result<usize> {
    // modified from rust stdlib file src/io/mod.rs

    let orig_len = buf.len();
    while !buf.is_empty() {
        match r.read(buf).await {
            Ok(0) => break,
            Ok(n) => {
                buf = &mut buf[n..];
            }
            Err(e) => return Err(e),
        }
    }
    Ok(orig_len - buf.len())
}
