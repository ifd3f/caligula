use std::io::{Read, Write};

use aligned_vec::avec_rt;

use super::utils::{CountWrite, FileSourceReader, try_read_exact};
use crate::{
    codec::compression::CompressionFormat,
    herder_api::{
        error::{DiskError, InputFileError, IoError},
        write_verify::{WVError, WVEvent},
    },
};

/// Wraps a bunch of parameters for a big complicated operation where we:
///
/// - decompress the input file
/// - write to a disk
/// - write stats down a pipe
pub struct WriteOp<S: Read, D: Write> {
    /// File to read from
    pub file: S,
    /// Disk to write to
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

impl<S: Read, D: Write> WriteOp<S, D> {
    /// Execute the write operation. Returns total number of bytes written.
    #[inline(always)]
    pub fn execute(&mut self, mut tx: impl FnMut(WVEvent)) -> Result<u64, WVError> {
        let mut file = FileSourceReader::new(self.cf, self.file_read_buf_size, &mut self.file);
        let mut disk = CountWrite::new(&mut self.disk);
        let mut buf = avec_rt![[self.disk_block_size] | 0u8; self.buf_size];

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
                // Try to fill up the block if we can.
                let read_bytes =
                    try_read_exact(&mut file, &mut buf).map_err(IoError::<InputFileError>::from)?;
                if read_bytes == 0 {
                    disk.flush().map_err(IoError::<DiskError>::from)?;
                    checkpoint!();
                    return Ok(file.decompressed_bytes());
                }

                // Write the entire buffer, because we're doing direct writes.
                // Even if we didn't fill the whole buffer, we are still writing the whole
                // buffer.
                let written_bytes = disk.write(&buf[..]).map_err(IoError::<DiskError>::from)?;
                if written_bytes == 0 {
                    checkpoint!();
                    return Err(WVError::EndOfOutput);
                }
            }
            checkpoint!();
        }
    }
}
