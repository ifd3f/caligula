use std::io::{Read, Write};

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
