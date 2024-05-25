use base64::Engine;
use digest::Digest;
use serde::{Deserialize, Serialize};
use std::fmt::Display;
use std::io::Read;
use valuable::Valuable;

macro_rules! generate {
    {$(
        hash_length: $digest_bytes:expr => [
            $($enum_arm:ident {
                name: $sri_prefix:literal,
                display: $display:expr,
                new() -> $hash_inner:ty {
                    $makehash_expr:expr
                }
            })*
        ]
    )*} => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Valuable)]
        pub enum HashAlg {
            $($(
                $enum_arm,
            )*)*
        }

        /// Represents a hashing operation in progress.
        /// This is mostly useful to make a cute progress bar.
        pub struct Hashing<R>
        where
            R : Read,
        {
            inner: HashingInner<R>
        }

        enum HashingInner<R>
        where
            R : Read,
        {
            $($(
                $enum_arm(GenericHashing<$hash_inner, R>),
            )*)*
        }

        impl HashAlg {
            /// Parses from SRI algorithm prefix. See https://www.w3.org/TR/SRI/ for more information.
            /// Note that although SRI only supports sha256, sha384, and sha512, we parse out
            /// more than that, so it's not actually to spec, but who cares.
            pub fn from_sri_alg(alg: &str) -> Option<Self> {
                match alg {
                    $($(
                        $sri_prefix => Some(Self::$enum_arm),
                    )*)*
                    _ => None,
                }
            }

            /// Based on length of a hash, detects the possible hash algs
            /// this hash could be from.
            pub fn detect_from_length(bytes: usize) -> &'static [Self] {
                match bytes {
                    $(
                        $digest_bytes => &[
                            $(
                                Self::$enum_arm,
                            )*
                        ],
                    )*
                    _ => &[],
                }
            }

            /// Returns the digest size in bits.
            pub fn digest_bytes(&self) -> usize {
                match self {
                    $($(
                        Self::$enum_arm => $digest_bytes,
                    )*)*
                }
            }
        }

        impl Display for HashAlg {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                match self {
                    $($(
                        Self::$enum_arm => write!(f, $display),
                    )*)*
                }
            }
        }

        impl<R> Hashing<R>
        where
            R: Read,
        {
            #[inline]
            pub fn new(alg: HashAlg, r: R, block_size: usize) -> Self {
                let inner = match alg {
                    $($(
                        HashAlg::$enum_arm => HashingInner::$enum_arm(
                            GenericHashing::new($makehash_expr, r, block_size)
                        ),
                    )*)*
                };

                Self { inner }
            }

            #[inline]
            pub fn finalize(self) -> std::io::Result<FileHashInfo> {
                match self.inner {
                    $($(
                        HashingInner::$enum_arm(i) => i.finalize(),
                    )*)*
                }
            }

            #[inline]
            pub fn get_reader_mut(&mut self) -> &mut R {
                match &mut self.inner {
                    $($(
                        HashingInner::$enum_arm(i) => i.get_reader_mut(),
                    )*)*
                }
            }
        }

        impl<R> Iterator for Hashing<R>
        where
            R: Read,
        {
            type Item = usize;

            #[inline]
            fn next(&mut self) -> Option<Self::Item> {
                match &mut self.inner {
                    $($(
                        HashingInner::$enum_arm(i) => i.next(),
                    )*)*
                }
            }
        }
    }
}

generate! {
    hash_length: 16 => [
        Md5 {
            name: "md5",
            display: "MD5",
            new() -> md5::Md5 {
                md5::Md5::new()
            }
        }
    ]
    hash_length: 20 => [
        Sha1 {
            name: "sha1",
            display: "SHA-1",
            new() -> sha1::Sha1 {
                sha1::Sha1::new()
            }
        }
    ]
    hash_length: 28 => [
        Sha224 {
            name: "sha224",
            display: "SHA-224",
            new() -> sha2::Sha224 {
                sha2::Sha224::new()
            }
        }
    ]
    hash_length: 32 => [
        Sha256 {
            name: "sha256",
            display: "SHA-256",
            new() -> sha2::Sha256 {
                sha2::Sha256::new()
            }
        }
    ]
    hash_length: 48 => [
        Sha384 {
            name: "sha384",
            display: "SHA-384",
            new() -> sha2::Sha384 {
                sha2::Sha384::new()
            }
        }
    ]
    hash_length: 64 => [
        Sha512 {
            name: "sha512",
            display: "SHA-512",
            new() -> sha2::Sha512 {
                sha2::Sha512::new()
            }
        }
    ]
}

/// Represents a hashing operation in progress.
/// This is mostly useful to make a cute progress bar.
struct GenericHashing<H, R>
where
    H: Digest,
    R: Read,
{
    hash: H,
    read: R,
    len: usize,
    buf: Vec<u8>,
    error: Option<std::io::Error>,
}

/// Represents the full results of hashing.
pub struct FileHashInfo {
    pub file_bytes: u64,
    pub file_hash: Vec<u8>,
}

impl<H, R> GenericHashing<H, R>
where
    H: Digest,
    R: Read,
{
    pub fn new(hash: H, read: R, block_size: usize) -> Self {
        Self {
            hash,
            read,
            len: 0,
            buf: vec![0; block_size],
            error: None,
        }
    }

    pub fn get_reader_mut(&mut self) -> &mut R {
        &mut self.read
    }

    pub fn finalize(self) -> std::io::Result<FileHashInfo> {
        match self.error {
            Some(e) => Err(e),
            None => Ok(FileHashInfo {
                file_bytes: self.len as u64,
                file_hash: self.hash.finalize()[..].into(),
            }),
        }
    }

    /// Performs one step. Returns how many bytes were read.
    /// Does not set the "failed" flag.
    fn step(&mut self) -> std::io::Result<usize> {
        let read_bytes = self.read.read(&mut self.buf)?;
        if read_bytes > 0 {
            self.hash.update(&self.buf[..read_bytes]);
        }
        self.len += read_bytes;
        Ok(read_bytes)
    }
}

impl<H, R> Iterator for GenericHashing<H, R>
where
    H: Digest,
    R: Read,
{
    type Item = usize;

    fn next(&mut self) -> Option<Self::Item> {
        if self.error.is_some() {
            return None;
        }

        match self.step() {
            Ok(0) => None,
            Ok(_) => Some(self.len),
            Err(e) => {
                self.error = Some(e);
                None
            }
        }
    }
}

pub fn parse_base16_or_base64(s: &str) -> Option<Vec<u8>> {
    base16::decode(s)
        .or_else(|_| base64::engine::general_purpose::URL_SAFE.decode(&s))
        .or_else(|_| base64::engine::general_purpose::STANDARD.decode(&s))
        .ok()
}

pub fn parse_hash_input(h: &str) -> Result<(Vec<HashAlg>, Vec<u8>), HashParseError> {
    if h.is_empty() {
        return Err(HashParseError::EmptyInput);
    }

    if let Some((alg, hash)) = h.split_once('-') {
        let alg =
            HashAlg::from_sri_alg(alg).ok_or_else(|| HashParseError::UnknownAlg(alg.into()))?;
        let expected_hash = parse_base16_or_base64(hash)
            .ok_or_else(|| HashParseError::SRIValueNotBase16OrBase64)?;

        let expected_bytes = alg.digest_bytes();
        let actual_bytes = expected_hash.len();
        if expected_bytes != actual_bytes {
            return Err(HashParseError::InvalidLengthForAlg {
                alg,
                expected_bytes,
                actual_bytes,
            });
        }

        return Ok((vec![alg], expected_hash));
    }

    if let Some(bytes) = parse_base16_or_base64(h) {
        let len = bytes.len();
        let alg = HashAlg::detect_from_length(len);
        if alg.is_empty() {
            return Err(HashParseError::AlgDetectionFailure(len));
        }

        return Ok((alg.into(), bytes));
    }

    Err(HashParseError::UnparseableInput)
}

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum HashParseError {
    #[error("Unknown algorithm {0}")]
    UnknownAlg(String),
    #[error("SRI-style value is not base16 or base64")]
    SRIValueNotBase16OrBase64,
    #[error("Algorithm {alg} expected a digest of length {expected_bytes}, but got length {actual_bytes}")]
    InvalidLengthForAlg {
        alg: HashAlg,
        expected_bytes: usize,
        actual_bytes: usize,
    },
    #[error("Could not detect hash algorithm from digest size {0}")]
    AlgDetectionFailure(usize),
    #[error(
        "Provided argument is not a hash algorithm, SRI-style hash, nor is it base16 or base64"
    )]
    UnparseableInput,
    #[error("Input is empty")]
    EmptyInput,
}

#[cfg(test)]
mod tests {
    use base64::Engine;

    use crate::hash::HashAlg;

    use super::{parse_hash_input, HashParseError};
    use test_case::test_case;

    #[test]
    fn parse_valid_sri_hash() {
        let result = parse_hash_input(
            "sha384-EVSTQN3/azprG1Anm3QDgpJLIm9Nao0Yz1ztcQTwFspd3yD65VohhpuuCOmLASjC",
        )
        .unwrap();

        assert_eq!(
            result,
            (
                vec![HashAlg::Sha384],
                base64::engine::general_purpose::STANDARD
                    .decode("EVSTQN3/azprG1Anm3QDgpJLIm9Nao0Yz1ztcQTwFspd3yD65VohhpuuCOmLASjC")
                    .unwrap()
            )
        )
    }

    #[test]
    fn parse_valid_sri_hash_base16() {
        let result = parse_hash_input("md5-b7fbc56aaec74706d8fdae71aae7b0ac").unwrap();

        assert_eq!(
            result,
            (
                vec![HashAlg::Md5],
                base16::decode("b7fbc56aaec74706d8fdae71aae7b0ac").unwrap()
            )
        )
    }

    #[test]
    fn parse_valid_base64_only_hash() {
        let result =
            parse_hash_input("EVSTQN3/azprG1Anm3QDgpJLIm9Nao0Yz1ztcQTwFspd3yD65VohhpuuCOmLASjC")
                .unwrap();

        assert_eq!(
            result,
            (
                vec![HashAlg::Sha384],
                base64::engine::general_purpose::STANDARD
                    .decode("EVSTQN3/azprG1Anm3QDgpJLIm9Nao0Yz1ztcQTwFspd3yD65VohhpuuCOmLASjC")
                    .unwrap()
            )
        )
    }

    #[test]
    fn parse_valid_base16_only_hash() {
        let result =
            parse_hash_input("531a1557d205e09358e16fc4d79911ae4b9e28984bf10dbd7ab42d39f6a10713")
                .unwrap();

        assert_eq!(
            result,
            (
                vec![HashAlg::Sha256],
                base16::decode("531a1557d205e09358e16fc4d79911ae4b9e28984bf10dbd7ab42d39f6a10713")
                    .unwrap()
            )
        );
    }

    #[test_case("asdf-fdsu" => HashParseError::UnknownAlg("asdf".into()); "bad algo")]
    #[test_case("sha256-deadbeef" => HashParseError::InvalidLengthForAlg{ alg: HashAlg::Sha256, expected_bytes: 32, actual_bytes: 4}; "bad length")]
    #[test_case("sha256-" => HashParseError::InvalidLengthForAlg { alg: HashAlg::Sha256, expected_bytes: 32, actual_bytes: 0 }; "sri no hash")]
    #[test_case("" => HashParseError::EmptyInput; "empty")]
    #[test_case("f9od:fd" => HashParseError::UnparseableInput; "invalid chars")]
    fn parse_invalid_hash(input: &str) -> HashParseError {
        parse_hash_input(input).unwrap_err()
    }
}
