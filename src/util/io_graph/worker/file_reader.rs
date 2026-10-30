use std::{
    fs::File,
    io::Read,
    path::{Path, PathBuf},
};

use crate::util::{
    alloc_uninit_bytes_with_layout,
    io_graph::{self, ALLOC_LAYOUT, GraphContext, SendBytes, Worker},
};

/// A worker optimized for reading a file on disk.
///
/// It uses boring `read()` calls.
pub fn file_reader(
    path: &Path,
    mut tx: impl SendBytes,
) -> Result<
    (
        std::fs::Metadata,
        impl Worker<Output = (), Error = std::io::Error>,
    ),
    std::io::Error,
> {
    let mut file = File::open(path)?;
    let metadata = file.metadata()?;

    /*
    nix::fcntl::posix_fadvise(&file, 0, 0, PosixFadviseAdvice::POSIX_FADV_SEQUENTIAL)
        .ok_or_log();
    */

    let w = move |ctx: &GraphContext| {
        while !ctx.halt() {
            // SAFETY: these bytes will get filled up immediately. everything
            // else that wasn't filled up will get truncated
            let mut buf = unsafe { alloc_uninit_bytes_with_layout(ALLOC_LAYOUT) };

            let count = file.read(&mut buf)?;
            if count == 0 {
                break;
            }

            buf.truncate(count);
            tx.send(buf.freeze())?;
        }

        tx.close()?;

        Ok(())
    };

    Ok((metadata, w))
}
