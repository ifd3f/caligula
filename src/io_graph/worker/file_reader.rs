use std::{
    fs::File,
    path::{Path, PathBuf},
};

use crate::{
    io_graph::{ALLOC_LAYOUT, SendBytes, Worker, worker::Reader},
    util::alloc_uninit_bytes_with_layout,
};

/// A worker optimized for reading a file on disk.
pub struct FileReader {
    path: PathBuf,
    /// Max number of bytes to read from the file
    size: u64,
    file: File,
}

impl FileReader {
    /// Create a new FileReader with a maximum number of bytes to read.
    pub fn new(path: &Path, max_size: Option<u64>) -> std::io::Result<Box<Self>> {
        let file = File::open(path)?;
        let metadata = file.metadata()?;
        let size = max_size.unwrap_or(metadata.len());

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
        context: &crate::io_graph::GraphContext,
        args: Tx,
    ) -> Result<Self::Output, Self::Error> {
        let r = Reader::new(self.size.into());
        r.run(context, (&mut self.file, args))?;
        Ok(())
    }
}
