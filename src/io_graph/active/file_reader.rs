use std::{
    fs::File,
    path::{Path, PathBuf},
    sync::Arc,
};

use bytes::Bytes;
use memmap2::{Advice, Mmap, MmapOptions};
use tracing_unwrap::ResultExt;

use crate::io_graph::{SendBytes, Worker};

/// A worker optimized for reading a file on disk.
pub struct FileReader<Tx: SendBytes> {
    path: PathBuf,
    size: u64,
    read_size: usize,
    output: Tx,
    file: File,
    mmap: Mmap,
}

impl<Tx: SendBytes + Send> FileReader<Tx> {
    pub fn new(path: &Path, tx: Tx) -> std::io::Result<Box<Self>> {
        let file = File::open(path)?;

        let opts = MmapOptions::new();

        let mmap = unsafe { opts.map(&file)? };

        mmap.advise(Advice::Sequential).ok_or_log();

        Ok(Box::new(Self {
            output: tx,
            size: mmap.len() as u64,
            path: path.to_owned(),
            mmap,
            file,
            read_size: 65536 * 4,
        }))
    }

    /// Size of the file we're reading.
    pub fn size(&self) -> u64 {
        self.size
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn read_size(&self) -> usize {
        self.read_size
    }
}

impl<Tx: SendBytes + Send> Worker for FileReader<Tx> {
    type Error = std::io::Error;
    type Output = ();

    fn run(
        mut self: Box<Self>,
        context: &crate::io_graph::GraphContext,
    ) -> Result<Self::Output, Self::Error> {
        let mmap = Arc::new(self.mmap);
        let mut big_bytes = Bytes::from_owner(ArcMmapAsRefWrapper(mmap.clone()));

        let mut offset = 0;
        while !big_bytes.is_empty() && !context.halt() {
            let out = if big_bytes.len() <= self.read_size {
                // Just take what's left and ship it
                std::mem::take(&mut big_bytes)
            } else {
                // Push self forward, return what we skipped
                big_bytes.split_to(self.read_size)
            };

            // Force a read of the chunk we just took
            mmap.advise_range(Advice::PopulateRead, offset, out.len())?;

            offset += out.len();

            self.output.send(out)?;
        }

        self.output.close()?;

        Ok(())
    }
}

/// Because [`Bytes::from_owner`] doesn't like raw [`Arc<Mmap>`]s.
struct ArcMmapAsRefWrapper(Arc<Mmap>);

impl AsRef<[u8]> for ArcMmapAsRefWrapper {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}
