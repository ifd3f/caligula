use std::{
    fs::{File, OpenOptions},
    path::Path,
};

use crate::compression::CompressionFormat;

#[cfg(target_os = "linux")]
pub fn open_blockdev(path: impl AsRef<Path>, _cf: CompressionFormat) -> std::io::Result<File> {
    use std::os::unix::fs::OpenOptionsExt;

    use libc::O_DIRECT;

    let mut opts = OpenOptions::new();
    opts.write(true);

    // Write directly to the disk, full send, bypass the goddamn cache
    opts.custom_flags(O_DIRECT);

    opts.open(path)
}

#[cfg(target_os = "macos")]
pub fn open_blockdev(path: impl AsRef<Path>, _cf: CompressionFormat) -> std::io::Result<File> {
    // For more info, see:
    // https://stackoverflow.com/questions/2299402/how-does-one-do-raw-io-on-mac-os-x-ie-equivalent-to-linuxs-o-direct-flag

    use libc::{fcntl, F_NOCACHE, O_SYNC};
    use std::os::{fd::AsRawFd, unix::fs::OpenOptionsExt};

    let file = OpenOptions::new()
        .write(true)
        .custom_flags(O_SYNC)
        .open(path)?;

    #[cfg(target_os = "macos")]
    unsafe {
        fcntl(file.as_raw_fd().into(), F_NOCACHE);
    }

    Ok(file)
}
