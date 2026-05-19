use std::{
    fs::{File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};

use crate::io_graph::{RecvBytes, Worker};

/// Standard disk writer using normal write() calls.
pub struct StdDiskWriter {
    file: File,
    path: PathBuf,
    size: u64,
}

impl StdDiskWriter {
    pub fn new(path: &Path) -> std::io::Result<Box<Self>> {
        let file = open_blockdev(path)?;
        let size = file.metadata()?.len();

        /*
        nix::fcntl::posix_fadvise(&file, 0, 0, PosixFadviseAdvice::POSIX_FADV_SEQUENTIAL)
            .ok_or_log();
        */

        Ok(Box::new(Self {
            path: path.to_owned(),
            file,
            size,
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

impl<Rx: RecvBytes> Worker<Rx> for StdDiskWriter {
    type Error = std::io::Error;
    type Output = ();

    fn run(
        mut self: Box<Self>,
        _context: &crate::io_graph::GraphContext,
        args: Rx,
    ) -> Result<Self::Output, Self::Error> {
        let mut rx = args;

        while let Some(bs) = rx.recv()? {
            self.file.write_all(&bs)?;
        }

        Ok(())
    }
}

#[cfg(target_os = "linux")]
fn open_blockdev(path: impl AsRef<Path>) -> std::io::Result<File> {
    use std::os::unix::fs::OpenOptionsExt;

    use libc::O_DIRECT;

    let mut opts = OpenOptions::new();
    opts.write(true).custom_flags(O_DIRECT);

    opts.open(path)
}

#[cfg(target_os = "macos")]
fn open_blockdev(path: impl AsRef<Path>) -> std::io::Result<File> {
    // For more info, see:
    // https://stackoverflow.com/questions/2299402/how-does-one-do-raw-io-on-mac-os-x-ie-equivalent-to-linuxs-o-direct-flag

    use std::os::fd::AsRawFd;

    use libc::{F_NOCACHE, fcntl};

    let file = OpenOptions::new().write(true).open(path)?;

    unsafe {
        // Enable direct writes
        fcntl(file.as_raw_fd(), F_NOCACHE);
    }

    Ok(file)
}
