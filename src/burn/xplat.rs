use std::{
    fs::{File, OpenOptions},
    path::Path,
};

#[cfg(any(target_os = "macos", target_os = "linux"))]
pub fn open_blockdev(path: impl AsRef<Path>) -> std::io::Result<File> {
    use nix::fcntl::OFlag;
    use std::os::unix::fs::OpenOptionsExt;

    #[cfg(target_os = "macos")]
    let oflags = OFlag::O_SYNC;

    #[cfg(target_os = "linux")]
    let oflag = OFlag::O_DIRECT | OFlag::O_SYNC;

    OpenOptions::new()
        .write(true)
        .custom_flags(oflag.bits())
        .open(path)
}
