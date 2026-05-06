use base64::Engine;
use digest::Digest;
use serde::{Deserialize, Serialize};
use std::fmt::Display;

macro_rules! generate {
    {
        $($enum_arm:ident($hash_inner:ty) {
            name: $sri_prefix:literal,
            display: $display:expr,
        })*
    } => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
        pub enum HashAlg {
            $($enum_arm,)*
        }

        impl HashAlg {
            /// Parses from SRI algorithm prefix. See https://www.w3.org/TR/SRI/ for more information.
            /// Note that although SRI only supports sha256, sha384, and sha512, we parse out
            /// more than that, so it's not actually to spec, but who cares.
            pub fn from_sri_alg(alg: &str) -> Option<Self> {
                match alg {
                    $(
                        $sri_prefix => Some(Self::$enum_arm),
                    )*
                    _ => None,
                }
            }

            /// Returns the digest size in bytes.
            pub fn digest_bytes(&self) -> usize {
                match self {
                    $(Self::$enum_arm => <$hash_inner as Digest>::output_size(),)*
                }
            }

            pub fn values() -> &'static [Self] {
                &[$(Self::$enum_arm,)*]
            }
        }

        impl Display for HashAlg {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                match self {
                    $(
                        Self::$enum_arm => write!(f, $display),
                    )*
                }
            }
        }

        macro_rules! with_hasher {
            {$alg:expr => $hasher_bind:ident: $action:expr } => {
                match $alg {
                    $(crate::hash::HashAlg::$enum_arm => {
                        type $hasher_bind = $hash_inner;
                        $action
                    })*
                }
            }
        }

        pub(crate) use with_hasher;
    }
}

impl HashAlg {
    /// Based on length of a hash, detects the possible hash algs
    /// this hash could be from.
    pub fn detect_from_length(bytes: usize) -> Vec<Self> {
        Self::values()
            .iter()
            .copied()
            .filter(|alg| alg.digest_bytes() == bytes)
            .collect()
    }
}

generate! {
    Md5(::md5::Md5) {
        name: "md5",
        display: "MD5",
    }
    Sha1(::sha1::Sha1) {
        name: "sha1",
        display: "SHA-1",
    }
    Sha224(::sha2::Sha224) {
        name: "sha224",
        display: "SHA-224",
    }
    Sha256(::sha2::Sha256) {
        name: "sha256",
        display: "SHA-256",
    }
    Sha384(::sha2::Sha384) {
        name: "sha384",
        display: "SHA-384",
    }
    Sha512(::sha2::Sha512) {
        name: "sha512",
        display: "SHA-512",
    }
}

/// Represents the full results of hashing.
pub struct FileHashInfo {
    pub file_hash: Vec<u8>,
}

pub fn parse_base16_or_base64(s: &str) -> Option<Vec<u8>> {
    base16::decode(s)
        .or_else(|_| base64::engine::general_purpose::URL_SAFE.decode(s))
        .or_else(|_| base64::engine::general_purpose::STANDARD.decode(s))
        .ok()
}

pub fn parse_hash_input(h: &str) -> Result<(Vec<HashAlg>, Vec<u8>), HashParseError> {
    if h.is_empty() {
        return Err(HashParseError::EmptyInput);
    }

    if let Some((alg, hash)) = h.split_once('-') {
        let alg =
            HashAlg::from_sri_alg(alg).ok_or_else(|| HashParseError::UnknownAlg(alg.into()))?;
        let expected_hash =
            parse_base16_or_base64(hash).ok_or(HashParseError::SRIValueNotBase16OrBase64)?;

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
    #[error(
        "Algorithm {alg} expected a digest of length {expected_bytes}, but got length {actual_bytes}"
    )]
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
    use test_case::test_case;

    use super::{HashParseError, parse_hash_input};
    use crate::hash::HashAlg;

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
