use std::{
    fs::File,
    io::Write,
    path::{Path, PathBuf},
};

use crate::util::io_graph::{
    self, RecvBytes, Worker,
    sys::{DiskMode, open_blockdev},
};

/// A worker optimized for writing to a file or disk.
///
/// It uses boring `write()` and `pwrite()` calls.
pub struct FileWriter {
    path: PathBuf,
    size: u64,
    file: File,
}

impl FileWriter {
    pub fn new(path: &Path) -> std::io::Result<Box<Self>> {
        let file = open_blockdev(path, DiskMode::Write)?;
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

    /// Size of the disk we're writing to.
    pub fn size(&self) -> u64 {
        self.size
    }

    #[expect(unused)]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl<Rx: RecvBytes> Worker<Rx> for FileWriter {
    type Error = std::io::Error;
    type Output = ();

    fn run(
        mut self: Box<Self>,
        context: &io_graph::GraphContext,
        args: Rx,
    ) -> Result<Self::Output, Self::Error> {
        let mut rx = args;

        // TODO: multithread this mf
        while !context.halt()
            && let Some(buf) = rx.recv()?
        {
            // Even for disks, this is fine if the input arrives in exact chunks.
            self.file.write(&buf)?;
        }

        Ok(())
    }
}
