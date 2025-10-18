use std::{
    fs::File,
    io::{BufReader, Read, Seek, Write},
};

use crate::compression::{CompressionFormat, DecompressRead, decompress};

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

    #[inline(always)]
    pub fn get_ref(&self) -> &R {
        &self.r
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

/// [`File::flush`] is a lie. It does literally nothing on most OSes. This is a
/// simple wrapper over [`File`] that:
///
/// - trivially delegates [`Read`] and [`Seek`]
/// - trivially delegates [`Write::write`]
/// - replaces [`Write::flush`] with the platform-specific synchronous call to ensure
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

/// A reader type specifically for [`super::WriteOp`] and [`super::VerifyOp`] to
/// read stuff off of files.
///
/// It provides decompression, buffering, and instrumentation of read stats.
pub struct FileSourceReader<R: Read>(CountRead<DecompressRead<BufReader<CountRead<R>>>>);

impl<R: Read> FileSourceReader<R> {
    #[inline(always)]
    pub fn new(cf: CompressionFormat, buf_size: usize, r: R) -> Self {
        FileSourceReader(CountRead::new(
            decompress(cf, BufReader::with_capacity(buf_size, CountRead::new(r))).unwrap(),
        ))
    }

    /// How many bytes we've read from the file. In other words, pre-decompression size.
    #[inline(always)]
    pub fn read_file_bytes(&self) -> u64 {
        self.0.get_ref().get_ref().get_ref().count()
    }

    /// How many bytes we've read after decompression.
    #[inline(always)]
    pub fn decompressed_bytes(&self) -> u64 {
        self.0.count()
    }
}

impl<R: Read> Read for FileSourceReader<R> {
    #[inline(always)]
    fn read(&mut self, buf: &mut [u8]) -> futures_io::Result<usize> {
        self.0.read(buf)
    }
}
