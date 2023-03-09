use std::{
    fmt::Display,
    io::{BufRead, Read},
};

use bzip2::bufread::BzDecoder;
use flate2::bufread::GzDecoder;
use serde::{Deserialize, Serialize};
use xz::bufread::XzDecoder;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CompressionFormat {
    Identity,
    Gzip,
    Bzip2,
    Xz,
}

impl CompressionFormat {
    pub fn detect_from_extension(ext: &str) -> Self {
        match ext.to_lowercase().strip_prefix(".") {
            Some(ext) => match ext {
                "gz" => Self::Gzip,
                "xz" => Self::Xz,
                "bz2" => Self::Bzip2,
                _ => Self::Identity,
            },
            None => Self::Identity,
        }
    }

    pub fn decompress(&self, r: impl BufRead) -> impl Read {
        match self {
            CompressionFormat::Identity => DecompressRead::Identity(r),
            CompressionFormat::Gzip => DecompressRead::Gzip(GzDecoder::new(r)),
            CompressionFormat::Bzip2 => DecompressRead::Bzip2(BzDecoder::new(r)),
            CompressionFormat::Xz => DecompressRead::Xz(XzDecoder::new(r)),
        }
    }
}

macro_rules! decompress_read {
    (
        $name:ident <$var:ident> {
            $( $enumname:ident ($inner:ty), )*
        }
    ) => {
        enum $name<$var> where $var : BufRead {
            $(
                $enumname($inner),
            )*
        }

        impl<R> Read for DecompressRead<R> where R : BufRead {
            fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
                match self {
                    $(
                        Self::$enumname(r) => r.read(buf),
                    )*
                }
            }
        }
    };
}

decompress_read!(
    DecompressRead<R> {
        Identity(R),
        Gzip(GzDecoder<R>),
        Bzip2(BzDecoder<R>),
        Xz(XzDecoder<R>),
    }
);

impl Display for CompressionFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CompressionFormat::Identity => write!(f, "no compression"),
            CompressionFormat::Gzip => write!(f, "gzip"),
            CompressionFormat::Bzip2 => write!(f, "bzip2"),
            CompressionFormat::Xz => write!(f, "xz/lzma"),
        }
    }
}