use std::{
    fs::{File, OpenOptions},
    path::Path,
};

#[derive(Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum DiskMode {
    Read,
    Write,
}

#[cfg(target_os = "linux")]
pub fn open_blockdev(path: impl AsRef<Path>, mode: DiskMode) -> std::io::Result<File> {
    use std::os::unix::fs::OpenOptionsExt;

    use libc::O_DIRECT;

    let mut opts = OpenOptions::new();
    opts.write(mode == DiskMode::Write)
        .read(mode == DiskMode::Read)
        .custom_flags(O_DIRECT);

    opts.open(path)
}

#[cfg(target_os = "macos")]
pub fn open_blockdev(path: impl AsRef<Path>, mode: DiskMode) -> std::io::Result<File> {
    // For more info, see:
    // https://stackoverflow.com/questions/2299402/how-does-one-do-raw-io-on-mac-os-x-ie-equivalent-to-linuxs-o-direct-flag

    use std::os::fd::AsRawFd;

    use libc::{F_NOCACHE, fcntl};

    let file = OpenOptions::new()
        .write(mode == DiskMode::Write)
        .read(mode == DiskMode::Read)
        .open(path)?;

    unsafe {
        // Enable direct writes
        fcntl(file.as_raw_fd(), F_NOCACHE);
    }

    Ok(file)
}
