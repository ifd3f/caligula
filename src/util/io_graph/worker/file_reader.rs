use std::{
    fs::File,
    io::Read,
    path::{Path, PathBuf},
};

use crate::util::{
    alloc_uninit_bytes_with_layout,
    io_graph::{self, ALLOC_LAYOUT, SendBytes, Worker},
};

/// A worker optimized for reading a file on disk.
pub struct FileReader {
    path: PathBuf,
    size: u64,
    file: File,
}

impl FileReader {
    pub fn new(path: &Path) -> std::io::Result<Box<Self>> {
        let file = File::open(path)?;
        let size = file.metadata()?.len();

        /*
        nix::fcntl::posix_fadvise(&file, 0, 0, PosixFadviseAdvice::POSIX_FADV_SEQUENTIAL)
            .ok_or_log();
        */

        Ok(Box::new(Self {
            size,
            path: path.to_owned(),
            file,
        }))
    }

    /// Size of the file we're reading.
    pub fn size(&self) -> u64 {
        self.size
    }

    #[expect(unused)]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl<Tx: SendBytes> Worker<Tx> for FileReader {
    type Error = std::io::Error;
    type Output = ();

    fn run(
        mut self: Box<Self>,
        context: &io_graph::GraphContext,
        args: Tx,
    ) -> Result<Self::Output, Self::Error> {
        let mut tx = args;

        while !context.halt() {
            // SAFETY: these bytes will get filled up immediately. everything else that
            // wasn't filled up will get truncated
            let mut buf = unsafe { alloc_uninit_bytes_with_layout(ALLOC_LAYOUT) };

            let count = self.file.read(&mut buf)?;
            if count == 0 {
                break;
            }

            buf.truncate(count);
            tx.send(buf.freeze())?;
        }

        tx.close()?;

        Ok(())
    }
}
