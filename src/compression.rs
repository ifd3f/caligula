use std::{
    fmt::Display,
    io::{BufRead, Read},
};

use serde::{Deserialize, Serialize};
use valuable::Valuable;

macro_rules! generate {
    {
        $readervar:ident: $r:ident {
            $(
                [feature=$feature:expr] $extpat:pat =>
                    $enumarm:ident($display:expr, $inner:ty)
                    $dcrinner:expr,
            )*
        }
    } => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Valuable)]
        pub enum CompressionFormat {
            Identity,
            $(
                $enumarm,
            )*
        }

        pub const AVAILABLE_FORMATS: &[CompressionFormat] = &[
            CompressionFormat::Identity,
            $(
                CompressionFormat::$enumarm,
            )*
        ];

        impl CompressionFormat {
            pub fn detect_from_extension(ext: &str) -> Self {
                match ext.to_lowercase().trim_start_matches(".") {
                    $(
                        $extpat => Self::$enumarm,
                    )*
                    _ => Self::Identity,
                }
            }

            pub fn is_identity(self) -> bool {
                match self {
                    Self::Identity => true,
                    _ => false,
                }
            }

            pub fn is_available(self) -> bool {
                match self {
                    Self::Identity => true,
                    $(
                        Self::$enumarm => cfg!(feature = $feature),
                    )*
                }
            }
        }

        impl Display for CompressionFormat {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                match self {
                    CompressionFormat::Identity => write!(f, "no compression"),
                    $(
                        Self::$enumarm => write!(f, $display),
                    )*
                }
            }
        }

        pub enum DecompressRead<$r> {
            Identity($r),
            $(
                #[cfg(feature = $feature)]
                $enumarm($inner),
            )*
        }

        impl<R> DecompressRead<R>
        where
            R: BufRead,
        {
            pub fn get_mut(&mut self) -> &mut R {
                match self {
                    Self::Identity(r) => r,
                    $(
                        #[cfg(feature = $feature)]
                        Self::$enumarm(r) => r.get_mut(),
                    )*
                }
            }
        }

        impl<R> Read for DecompressRead<R>
        where
            R: BufRead,
        {
            fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
                match self {
                    Self::Identity(r) => r.read(buf),
                    $(
                        #[cfg(feature = $feature)]
                        Self::$enumarm(r) => r.read(buf),
                    )*
                }
            }
        }

        pub fn decompress<R>(cf: CompressionFormat, $readervar: R) -> Result<DecompressRead<R>, DecompressError>
        where
            R : BufRead
        {
            match cf {
                CompressionFormat::Identity => Ok(DecompressRead::Identity($readervar)),
                $(
                    CompressionFormat::$enumarm => {
                        #[cfg(feature = $feature)]
                        let result = Ok(DecompressRead::$enumarm($dcrinner));

                        #[cfg(not(feature = $feature))]
                        let result = Err(DecompressError::UnsupportedFormat(
                            CompressionFormat::$enumarm
                        ));

                        result
                    }
                )*
            }
        }
    }
}

generate! {
    r: R {
        [feature = "gz"] "gz" => Gzip("gzip", flate2::bufread::GzDecoder<R>) {
            flate2::bufread::GzDecoder::new(r)
        },
        [feature = "bz2"] "bz2" => Bzip2("bzip2", bzip2::bufread::BzDecoder<R>) {
            bzip2::bufread::BzDecoder::new(r)
        },
        [feature = "xz"] "xz" => Xz("xz/LZMA", xz2::bufread::XzDecoder<R>) {
            xz2::bufread::XzDecoder::new(r)
        },
    }
}

#[derive(Debug, thiserror::Error)]
pub enum DecompressError {
    #[allow(unused)]
    #[error("Unsupported compression format {0}!")]
    UnsupportedFormat(CompressionFormat),
}
