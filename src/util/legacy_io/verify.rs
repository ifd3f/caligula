use std::io::Read;

use aligned_vec::avec_rt;

use crate::{
    codec::compression::CompressionFormat,
    herder_api::{
        error::{DiskError, InputFileError, IoError},
        write_verify::{WVError, WVEvent},
    },
    util::legacy_io::utils::{CountRead, FileSourceReader, try_read_exact},
};

/// Wraps a bunch of parameters for a big complicated operation where we:
///
/// - decompress the input file
/// - read from a disk
/// - verify both sides are correct
/// - write stats down a pipe
pub struct VerifyOp<S: Read, D: Read> {
    /// File to validate against
    pub file: S,
    /// Disk to validate
    pub disk: D,
    /// Compression format to use
    pub cf: CompressionFormat,
    /// Buffer size to use when writing
    pub buf_size: usize,
    /// Block size of the disk
    pub disk_block_size: usize,
    /// How many writes of size [`Self::buf_size`] before we report back
    pub checkpoint_period: usize,
    /// How big the file reader's buffer should be
    pub file_read_buf_size: usize,
}

impl<S: Read, D: Read> VerifyOp<S, D> {
    #[inline(always)]
    pub fn execute(&mut self, mut tx: impl FnMut(WVEvent)) -> Result<(), WVError> {
        let mut file = FileSourceReader::new(self.cf, self.file_read_buf_size, &mut self.file);
        let mut disk = CountRead::new(&mut self.disk);

        let mut file_buf = avec_rt![[self.disk_block_size] | 0u8; self.buf_size];
        let mut disk_buf = avec_rt![[self.disk_block_size] | 0u8; self.buf_size];

        macro_rules! checkpoint {
            () => {
                tx(WVEvent::TotalBytes {
                    src: file.read_file_bytes(),
                    dest: disk.count(),
                });
            };
        }

        loop {
            for _ in 0..self.checkpoint_period {
                let file_read_bytes = try_read_exact(&mut file, &mut file_buf)
                    .map_err(IoError::<InputFileError>::from)?;
                if file_read_bytes == 0 {
                    checkpoint!();
                    return Ok(());
                }

                try_read_exact(&mut disk, &mut disk_buf).map_err(IoError::<DiskError>::from)?;

                if file_buf[..file_read_bytes] != disk_buf[..file_read_bytes] {
                    tracing::warn!(file_read_bytes, "verification failed");
                    return Err(WVError::VerificationFailed);
                }
            }
            checkpoint!();
        }
    }
}
