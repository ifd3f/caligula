use std::path::Path;

#[cfg(target_os = "linux")]
use tokio::fs::File;

#[cfg(target_os = "linux")]
pub async fn open_blockdev(path: impl AsRef<Path>) -> std::io::Result<File> {
    use libc::O_DIRECT;
    use tokio::fs::OpenOptions;

    let mut opts = OpenOptions::new();
    opts.write(true).read(true).custom_flags(O_DIRECT);

    opts.open(path).await
}

#[cfg(target_os = "macos")]
pub async fn open_blockdev(path: impl AsRef<Path>) -> std::io::Result<File> {
    // For more info, see:
    // https://stackoverflow.com/questions/2299402/how-does-one-do-raw-io-on-mac-os-x-ie-equivalent-to-linuxs-o-direct-flag

    use libc::{F_NOCACHE, fcntl};
    use std::os::fd::AsRawFd;

    let file = OpenOptions::new().write(true).read(true).open(path)?;

    unsafe {
        // Enable direct writes
        fcntl(file.as_raw_fd(), F_NOCACHE);
    }

    Ok(file)
}
