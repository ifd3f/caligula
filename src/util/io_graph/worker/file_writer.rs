use std::{io::Write, path::Path};

use crate::util::io_graph::{
    GraphContext, RecvBytes, Worker,
    sys::{DiskMode, open_blockdev},
};

/// A worker optimized for writing to a file or disk.
///
/// It uses boring `write()` and `pwrite()` calls.
pub fn file_writer(
    path: &Path,
    mut rx: impl RecvBytes,
) -> Result<
    (
        std::fs::Metadata,
        impl Worker<Output = (), Error = std::io::Error>,
    ),
    std::io::Error,
> {
    let mut file = open_blockdev(path, DiskMode::Write)?;
    /*
    nix::fcntl::posix_fadvise(&file, 0, 0, PosixFadviseAdvice::POSIX_FADV_SEQUENTIAL)
        .ok_or_log();
    */

    let metadata = file.metadata()?;

    let w = move |ctx: &GraphContext| {
        // TODO: multithread this mf
        while !ctx.halt()
            && let Some(buf) = rx.recv()?
        {
            // Even for disks, this is fine if the input arrives in exact
            // chunks.
            file.write(&buf)?;
        }

        Ok(())
    };
    Ok((metadata, w))
}
