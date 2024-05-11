use std::{
    fs::File,
    io::{Read, Seek, Write},
};

/// Wraps a reader and counts how many bytes we've read in total, without
/// making any system calls.
pub struct CountRead<R: Read> {
    r: R,
    count: u64,
}

impl<R: Read> CountRead<R> {
    #[inline(always)]
    pub fn new(r: R) -> Self {
        Self { r, count: 0 }
    }

    #[inline(always)]
    pub fn count(&self) -> u64 {
        self.count
    }
}

impl<R: Read> Read for CountRead<R> {
    #[inline(always)]
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let bytes = self.r.read(buf)?;
        self.count += bytes as u64;
        Ok(bytes)
    }
}

/// Wraps a writer and counts how many bytes we've written in total, without
/// making any system calls.
pub struct CountWrite<W: Write> {
    w: W,
    count: u64,
}

impl<W: Write> CountWrite<W> {
    #[inline(always)]
    pub fn new(w: W) -> Self {
        Self { w, count: 0 }
    }

    #[inline(always)]
    pub fn count(&self) -> u64 {
        self.count
    }
}

impl<W: Write> Write for CountWrite<W> {
    #[inline(always)]
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let bytes = self.w.write(buf)?;
        self.count += bytes as u64;
        Ok(bytes)
    }

    #[inline(always)]
    fn flush(&mut self) -> std::io::Result<()> {
        self.w.flush()
    }
}

/// [`File::flush`] is a lie. It does literally nothing. This is a simple wrapper
/// over [`File`] that:
///
/// - trivially delegates Read and Seek
/// - trivially delegates Write::write
/// - replaces Write::flush with the platform-specific synchronous call to ensure
///   that the data has been written to the disk.
pub struct SyncDataFile(pub File);

impl Read for SyncDataFile {
    #[inline(always)]
    fn read(&mut self, buf: &mut [u8]) -> futures_io::Result<usize> {
        self.0.read(buf)
    }
}

impl Write for SyncDataFile {
    #[inline(always)]
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.write(buf)
    }

    #[inline(always)]
    fn flush(&mut self) -> std::io::Result<()> {
        #[cfg(target_os = "linux")]
        {
            self.0.sync_data()
        }

        // On MacOS, calling sync_data() on a disk yields "Inappropriate ioctl for device (os error 25)"
        // so for now we will just no-op.
        #[cfg(target_os = "macos")]
        {
            Ok(())
        }
    }
}

impl Seek for SyncDataFile {
    #[inline(always)]
    fn seek(&mut self, pos: futures_io::SeekFrom) -> std::io::Result<u64> {
        self.0.seek(pos)
    }
}
