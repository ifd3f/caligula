use std::{
    fmt::Display,
    io::{BufRead, Read},
};

use bzip2::bufread::BzDecoder;
use flate2::bufread::GzDecoder;
use serde::{Deserialize, Serialize};
use strum::EnumIter;
use valuable::Valuable;
use xz::bufread::XzDecoder;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, EnumIter, Valuable)]
pub enum CompressionFormat {
    Identity,
    Gzip,
    Bzip2,
    Xz,
}

impl CompressionFormat {
    pub fn detect_from_extension(ext: &str) -> Self {
        match ext.to_lowercase().trim_start_matches(".") {
            "gz" => Self::Gzip,
            "xz" => Self::Xz,
            "bz2" => Self::Bzip2,
            _ => Self::Identity,
        }
    }

    pub fn decompress<R>(self, r: R) -> DecompressRead<R>
    where
        R: BufRead,
    {
        match self {
            CompressionFormat::Identity => DecompressRead::Identity(r),
            CompressionFormat::Gzip => DecompressRead::Gzip(GzDecoder::new(r)),
            CompressionFormat::Bzip2 => DecompressRead::Bzip2(BzDecoder::new(r)),
            CompressionFormat::Xz => DecompressRead::Xz(XzDecoder::new(r)),
        }
    }

    pub fn is_identity(self) -> bool {
        match self {
            CompressionFormat::Identity => true,
            _ => false,
        }
    }
}

pub enum DecompressRead<R>
where
    R: BufRead,
{
    Identity(R),
    Gzip(GzDecoder<R>),
    Bzip2(BzDecoder<R>),
    Xz(XzDecoder<R>),
}

impl<R> DecompressRead<R>
where
    R: BufRead,
{
    pub fn get_mut(&mut self) -> &mut R {
        match self {
            DecompressRead::Identity(r) => r,
            DecompressRead::Gzip(r) => r.get_mut(),
            DecompressRead::Bzip2(r) => r.get_mut(),
            DecompressRead::Xz(r) => r.get_mut(),
        }
    }
}

impl<R> Read for DecompressRead<R>
where
    R: BufRead,
{
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self {
            DecompressRead::Identity(r) => r.read(buf),
            DecompressRead::Gzip(r) => r.read(buf),
            DecompressRead::Bzip2(r) => r.read(buf),
            DecompressRead::Xz(r) => r.read(buf),
        }
    }
}

impl Display for CompressionFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CompressionFormat::Identity => write!(f, "no compression"),
            CompressionFormat::Gzip => write!(f, "gzip"),
            CompressionFormat::Bzip2 => write!(f, "bzip2"),
            CompressionFormat::Xz => write!(f, "LZMA (xz)"),
        }
    }
}
