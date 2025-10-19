use tokio::io::AsyncRead;

/// A reader type specifically for [`super::WriteOp`] and [`super::VerifyOp`] to
/// read stuff off of files.
///
/// It provides decompression, buffering, and instrumentation of read stats.
pub struct FileSourceReader<R: AsyncRead>(
    AsyncCountRead<AsyncDecompressRead<BufReader<CountRead<R>>>>,
);

impl<R: AsyncRead> FileSourceReader<R> {
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
