mod zstd_streaming_decoder;

use clap::ValueEnum;
use std::{
    fmt::Display,
    io::{BufRead, Read},
    path::Path,
};

use serde::{Deserialize, Serialize};

macro_rules! generate {
    {
        reader_var: $reader_var:ident,
        reader_typename: $reader_typename:ident,
        $($enum_arm:ident {
            extension_pattern: $ext_pat:pat,
            display: $display:expr,
            from_reader() -> $inner:ty {
                $from_reader:expr
            }
        })*
    } => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
        pub enum CompressionArg {
            Ask,
            Auto,
            None,
            $(
                $enum_arm,
            )*
        }

        impl CompressionArg {
            /// Returns the associated actual format of this CompressionArg,
            /// or None if this is not associated with any specific format.
            pub fn associated_format(&self) -> Option<CompressionFormat> {
                match self {
                    Self::Ask => None,
                    Self::Auto => None,
                    Self::None => Some(CompressionFormat::Identity),
                    $(
                        Self::$enum_arm => Some(CompressionFormat::$enum_arm),
                    )*
                }
            }
        }

        #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
        pub enum CompressionFormat {
            Identity,
            $(
                $enum_arm,
            )*
        }

        pub const AVAILABLE_FORMATS: &[CompressionFormat] = &[
            CompressionFormat::Identity,
            $(
                CompressionFormat::$enum_arm,
            )*
        ];

        impl CompressionFormat {
            pub fn detect_from_extension(ext: &str) -> Self {
                match ext.to_lowercase().trim_start_matches(".") {
                    $(
                        $ext_pat => Self::$enum_arm,
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
        }

        impl Display for CompressionFormat {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                match self {
                    CompressionFormat::Identity => write!(f, "no compression"),
                    $(
                        Self::$enum_arm => write!(f, $display),
                    )*
                }
            }
        }

        pub enum DecompressRead<$reader_typename: BufRead> {
            Identity($reader_typename),
            $(
                $enum_arm($inner),
            )*
        }

        impl<R> DecompressRead<R>
        where
            R: BufRead,
        {
            pub fn get_ref(&self) -> &R {
                match self {
                    Self::Identity(r) => r,
                    $(
                        Self::$enum_arm(r) => r.get_ref(),
                    )*
                }
            }

            pub fn get_mut(&mut self) -> &mut R {
                match self {
                    Self::Identity(r) => r,
                    $(
                        Self::$enum_arm(r) => r.get_mut(),
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
                        Self::$enum_arm(r) => r.read(buf),
                    )*
                }
            }
        }

        /// Open a decompressor for the given reader.
        pub fn decompress<R>(cf: CompressionFormat, $reader_var: R) -> anyhow::Result<DecompressRead<R>>
        where
            R : BufRead
        {
            match cf {
                CompressionFormat::Identity => Ok(DecompressRead::Identity($reader_var)),
                $(
                    CompressionFormat::$enum_arm => {
                        Ok(DecompressRead::$enum_arm($from_reader))
                    }
                )*
            }
        }
    }
}

generate! {
    reader_var: r,
    reader_typename: R,
    Gz {
        extension_pattern: "gz",
        display: "gzip",
        from_reader() -> flate2::bufread::GzDecoder<R> {
            flate2::bufread::GzDecoder::new(r)
        }
    }
    Bz2 {
        extension_pattern: "bz2",
        display: "bzip2",
        from_reader() -> bzip2::bufread::BzDecoder<R> {
            bzip2::bufread::BzDecoder::new(r)
        }
    }
    Xz {
        extension_pattern: "xz",
        display: "xz/LZMA",
        from_reader() -> xz2::bufread::XzDecoder<R> {
            xz2::bufread::XzDecoder::new(r)
        }
    }
    Lz4 {
        extension_pattern: "lz4",
        display: "lz4",
        from_reader() -> lz4_flex::frame::FrameDecoder<R> {
            lz4_flex::frame::FrameDecoder::new(r)
        }
    }
    Zst {
        extension_pattern: "zst",
        display: "zstd/ZStandard",
        from_reader() -> self::zstd_streaming_decoder::StreamingDecoder<R, ruzstd::frame_decoder::FrameDecoder> {
            self::zstd_streaming_decoder::StreamingDecoder::new(r)?
        }
    }
}

impl CompressionFormat {
    pub fn detect_from_path(path: impl AsRef<Path>) -> Option<CompressionFormat> {
        if let Some(ext) = path.as_ref().extension() {
            Some(CompressionFormat::detect_from_extension(
                &ext.to_string_lossy(),
            ))
        } else {
            None
        }
    }
}
