use base64::Engine;
use digest::Digest;
use serde::{Deserialize, Serialize};
use std::fmt::Display;
use std::io::Read;
use valuable::Valuable;

macro_rules! generate {
    {
        $(
            $digest_bytes:expr => [
                $(
                    $sri_prefix:expr => $enumarm:ident($display:expr): $hash_inner:ty {
                        $makehash_expr:expr
                    }
                )*
            ]
        )*
    } => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Valuable)]
        pub enum HashAlg {
            $($(
                $enumarm,
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
                $enumarm(GenericHashing<$hash_inner, R>),
            )*)*
        }

        impl HashAlg {
            /// Parses from SRI algorithm prefix. See https://www.w3.org/TR/SRI/ for more information.
            /// Note that although SRI only supports sha256, sha384, and sha512, we parse out
            /// more than that, so it's not actually to spec, but who cares.
            pub fn from_sri_alg(alg: &str) -> Option<Self> {
                match alg {
                    $($(
                        $sri_prefix => Some(Self::$enumarm),
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
                                Self::$enumarm,
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
                        Self::$enumarm => $digest_bytes,
                    )*)*
                }
            }
        }

        impl Display for HashAlg {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                match self {
                    $($(
                        Self::$enumarm => write!(f, $display),
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
                        HashAlg::$enumarm => HashingInner::$enumarm(
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
                        HashingInner::$enumarm(i) => i.finalize(),
                    )*)*
                }
            }

            #[inline]
            pub fn get_reader_mut(&mut self) -> &mut R {
                match &mut self.inner {
                    $($(
                        HashingInner::$enumarm(i) => i.get_reader_mut(),
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
                        HashingInner::$enumarm(i) => i.next(),
                    )*)*
                }
            }
        }
    }
}

generate! {
    16 => [
        "md5" => Md5("MD5"): md5::Md5 {
            md5::Md5::new()
        }
    ]
    20 => [
        "sha1" => Sha1("SHA-1"): sha1::Sha1 {
            sha1::Sha1::new()
        }
    ]
    28 => [
        "sha224" => Sha224("SHA-224"): sha2::Sha224 {
            sha2::Sha224::new()
        }
    ]
    32 => [
        "sha256" => Sha256("SHA-256"): sha2::Sha256 {
            sha2::Sha256::new()
        }
    ]
    48 => [
        "sha384" => Sha384("SHA-384"): sha2::Sha384 {
            sha2::Sha384::new()
        }
    ]
    64 => [
        "sha512" => Sha512("SHA-512"): sha2::Sha512 {
            sha2::Sha512::new()
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

pub fn guess_hashalg_from_str(s: &str) -> Option<(Vec<u8>, &'static [HashAlg])> {
    let decode = parse_base16_or_base64(s);

    decode.map(|b| {
        let alg = guess_hashalg_from_bytes(&b);
        (b, alg)
    })
}

pub fn guess_hashalg_from_bytes(b: &[u8]) -> &'static [HashAlg] {
    HashAlg::detect_from_length(b.len())
}

pub fn parse_base16_or_base64(s: &str) -> Option<Vec<u8>> {
    base16::decode(s)
        .or_else(|_| base64::engine::general_purpose::URL_SAFE.decode(&s))
        .or_else(|_| base64::engine::general_purpose::STANDARD.decode(&s))
        .ok()
}
