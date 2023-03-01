use std::{
    fs::File,
    io::{Read, Write},
    time::{Duration, Instant},
};

use bytesize::ByteSize;
use tokio::{
    sync::{broadcast, watch},
    task::{spawn_blocking, JoinHandle},
};

#[derive(Debug)]
pub struct BurnThread {
    src: File,
    dest: File,
}

#[derive(Debug)]
pub struct Writing {
    pub bytes_total: ByteSize,
    cursor_rx: watch::Receiver<WrittenBytes>,
    status_rx: broadcast::Receiver<StatusMessage>,
    thread: JoinHandle<anyhow::Result<()>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct WrittenBytes(usize);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StatusMessage {
    BlockSizeChanged(u64),
    BlockSizeSpeedInfo {
        blocks_written: usize,
        block_size: usize,
        duration: Duration,
    },
}

impl BurnThread {
    pub fn new(src: File, dest: File) -> anyhow::Result<Self> {
        Ok(Self { src, dest })
    }

    pub async fn start_write(self) -> anyhow::Result<Writing> {
        let bytes_total = self.src.metadata()?.len();

        let (cursor_tx, cursor_rx) = watch::channel(WrittenBytes::default());
        let (status_tx, status_rx) = broadcast::channel(32);

        let thread = spawn_blocking(move || self.write_worker(cursor_tx, status_tx));

        Ok(Writing {
            bytes_total: ByteSize::b(bytes_total),
            cursor_rx,
            status_rx,
            thread,
        })
    }

    fn write_worker(
        mut self,
        cursor_tx: watch::Sender<WrittenBytes>,
        status_tx: broadcast::Sender<StatusMessage>,
    ) -> anyhow::Result<()> {
        let block_size = ByteSize::kb(128).as_u64() as usize;
        let mut full_block = vec![0; block_size];

        let mut written_bytes: usize = 0;

        let stat_checkpoints: usize = 128;
        let checkpoint_blocks: usize = 128;

        cursor_tx.send(WrittenBytes(0))?;

        'outer: loop {
            let start = Instant::now();
            for _ in 0..stat_checkpoints {
                for _ in 0..checkpoint_blocks {
                    let read_bytes = self.src.read(&mut full_block)?;
                    if read_bytes == 0 {
                        break 'outer;
                    }

                    let write_bytes = self.dest.write(&full_block[..read_bytes])?;
                    written_bytes += write_bytes;
                    if written_bytes == 0 {
                        break 'outer;
                    }
                    self.dest.flush()?;
                }
                cursor_tx.send(WrittenBytes(written_bytes))?;
            }

            let duration = Instant::now().duration_since(start);
            status_tx.send(StatusMessage::BlockSizeSpeedInfo {
                blocks_written: checkpoint_blocks,
                block_size,
                duration,
            })?;
        }

        cursor_tx.send(WrittenBytes(written_bytes))?;

        Ok(())
    }
}
