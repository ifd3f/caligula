use std::{
    fs::File,
    io::Read,
    path::{Path, PathBuf},
};

use bytes::BytesMut;

use crate::io_graph::{SendBytes, Worker};

/// A worker optimized for reading a file on disk.
pub struct FileReader {
    path: PathBuf,
    size: u64,
    read_size: usize,
    file: File,
}

impl FileReader {
    pub fn new(path: &Path, read_size: usize) -> std::io::Result<Box<Self>> {
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
            read_size,
        }))
    }

    /// Size of the file we're reading.
    pub fn size(&self) -> u64 {
        self.size
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl<Tx: SendBytes> Worker<Tx> for FileReader {
    type Error = std::io::Error;
    type Output = ();

    fn run(
        mut self: Box<Self>,
        context: &crate::io_graph::GraphContext,
        args: Tx,
    ) -> Result<Self::Output, Self::Error> {
        let mut tx = args;

        while !context.halt() {
            let mut buf = BytesMut::with_capacity(self.read_size);

            // SAFETY: We are going to overwrite these bytes immediately.
            // The bytes we don't read will get trimmed down to size.
            // If you're concerned that the `File` impl may read these bytes, that's just
            // way too paranoid.
            unsafe {
                buf.set_len(self.read_size);
            }

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
