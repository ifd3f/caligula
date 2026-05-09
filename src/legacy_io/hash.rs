use std::io::{BufReader, Read, Seek};

use bytesize::ByteSize;

use crate::{
    compression::{CompressionFormat, DecompressionError},
    hash::{FileHashInfo, HashAlg, Hashing},
};

#[derive(Debug, thiserror::Error)]
pub enum HashingError {
    #[error("Decompression failed")]
    Decompress(#[from] DecompressionError),
    #[error("Failed to read file")]
    File(std::io::Error),
    #[error("Failed to calculate hash")]
    Hash(std::io::Error),
}

pub fn do_file_hashing(
    file: impl Read + Seek,
    cf: CompressionFormat,
    alg: HashAlg,
    mut checkpoint: impl FnMut(u64),
) -> Result<FileHashInfo, HashingError> {
    let decompress = crate::compression::decompress(cf, BufReader::new(file))?;

    let mut hashing = Hashing::new(
        alg,
        decompress,
        ByteSize::kib(512).as_u64() as usize, // TODO
    );
    loop {
        for _ in 0..32 {
            match hashing.next() {
                Some(_) => {}
                None => return Ok(hashing.finalize().map_err(HashingError::Hash)?),
            }
        }
        checkpoint(
            hashing
                .get_reader_mut()
                .get_mut()
                .stream_position()
                .map_err(HashingError::File)?,
        );
    }
}
