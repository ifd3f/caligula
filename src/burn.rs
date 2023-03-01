use std::{
    fs::File,
    io::{Read, Write},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

use bytesize::ByteSize;
use tokio::{
    sync::broadcast,
    task::{spawn_blocking, JoinHandle},
};

#[derive(Debug)]
pub struct BurnThread {
    dest: File,
    src: File,
}

#[derive(Debug)]
pub struct Writing {
    bytes_total: ByteSize,
    written_bytes: Arc<AtomicU64>,
    status_rx: broadcast::Receiver<StatusMessage>,
    thread: Option<JoinHandle<anyhow::Result<()>>>,
}

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
    pub fn new(dest: File, src: File) -> Self {
        Self { dest, src }
    }

    pub fn start_write(self) -> anyhow::Result<Writing> {
        let bytes_total = self.src.metadata()?.len();

        let (status_tx, status_rx) = broadcast::channel(32);

        let written_bytes: Arc<AtomicU64> = Arc::new(0.into());

        let thread_written = written_bytes.clone();
        let thread = spawn_blocking(move || self.write_worker(thread_written, status_tx));

        Ok(Writing {
            bytes_total: ByteSize::b(bytes_total),
            written_bytes,
            status_rx,
            thread: Some(thread),
        })
    }

    fn write_worker(
        mut self,
        report_written_bytes: Arc<AtomicU64>,
        status_tx: broadcast::Sender<StatusMessage>,
    ) -> anyhow::Result<()> {
        let block_size = ByteSize::kb(128).as_u64() as usize;
        let mut full_block = vec![0; block_size];

        let mut written_bytes: usize = 0;

        let stat_checkpoints: usize = 128;
        let checkpoint_blocks: usize = 128;

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
                report_written_bytes.store(written_bytes as u64, Ordering::Relaxed);
            }

            let duration = Instant::now().duration_since(start);
            status_tx.send(StatusMessage::BlockSizeSpeedInfo {
                blocks_written: checkpoint_blocks,
                block_size,
                duration,
            })?;
        }

        report_written_bytes.store(written_bytes as u64, Ordering::Relaxed);

        Ok(())
    }
}

impl Writing {
    pub fn bytes_total(&self) -> ByteSize {
        self.bytes_total
    }

    pub fn written_bytes(&self) -> ByteSize {
        ByteSize::b(self.written_bytes.load(Ordering::Relaxed))
    }

    pub async fn get_update(&mut self) -> Result<StatusMessage, broadcast::error::RecvError> {
        Ok(self.status_rx.recv().await?)
    }

    pub async fn join(&mut self) -> anyhow::Result<()> {
        let thread = self.thread.take();
        match thread {
            Some(thread) => {
                let res = thread.await?;
                Ok(res?)
            }
            None => Ok(()),
        }
    }
}
