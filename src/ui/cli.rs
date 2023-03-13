use std::path::{Path, PathBuf};

use clap::{Parser, Subcommand, ValueEnum};

use crate::{
    compression::CompressionFormat,
    hash::{parse_base16_or_base64, HashAlg},
};

/// A safe, user-friendly disk imager.
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
#[command(propagate_version = true)]
pub struct Args {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    Burn(BurnArgs),
}

/// Burn an image to a disk.
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct BurnArgs {
    /// Input file to burn.
    #[arg(value_parser = parse_path_exists)]
    pub input: PathBuf,

    /// Where to write the output. If not supplied, we will search for possible
    /// disks and ask you for where you want to burn.
    #[arg(short, value_parser = parse_path_exists)]
    pub out: Option<PathBuf>,

    /// If supplied, we will not ask for confirmation before destroying your disk.
    #[arg(short, long)]
    pub force: bool,

    /// If provided, we will not only show you removable disks, but all disks.
    /// If you use this option, please proceed with caution!
    #[arg(long)]
    pub show_all_disks: bool,

    /// What compression format the input file is. If `auto`, then we will guess
    /// based on the extension.
    #[arg(short = 'z', long, default_value = "auto")]
    pub compression: CompressionArg,

    /// The hash of the input file.
    /// 
    /// This can be provided in one of several formats:
    /// 
    ///  - `ask` to ask the user for a hash
    /// 
    ///  - `skip` or `none` to not do hash verification
    /// 
    ///  - an SRI-like string with either base16 or base64 in the format of `<alg>-<hash>` (i.e. `sha256-EVSTQN3/azprGF...`)
    /// 
    ///  - just a hash value, and we will guess the algorithm (i.e. `EVSTQN3/azprGF...`)
    /// 
    /// The following algorithms are supported: md5, sha1, sha224, sha256, sha384, sha512
    #[arg(short = 's', long, value_parser = parse_hash_arg, default_value = "ask")]
    pub hash: HashArg,
}

#[derive(Debug, Clone, PartialEq, Eq, ValueEnum)]
pub enum CompressionArg {
    Auto,
    None,
    Bz2,
    Gz,
    Xz,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HashArg {
    Ask,
    Skip,
    Hash {
        alg: Vec<HashAlg>,
        expected_hash: Vec<u8>,
    },
}

fn parse_path_exists(p: &str) -> Result<PathBuf, String> {
    let path = PathBuf::from(p);
    if !path.exists() {
        return Err(format!("path does not exist"));
    }
    Ok(path)
}

fn parse_hash_arg(h: &str) -> Result<HashArg, String> {
    match h.to_lowercase().as_ref() {
        "ask" => return Ok(HashArg::Ask),
        "skip" | "none" => return Ok(HashArg::Skip),
        _ => (),
    }
    if let Some((alg, hash)) = h.split_once('-') {
        let alg = HashAlg::from_sri_alg(alg).ok_or_else(|| format!("Invalid alg {alg}"))?;
        let expected_hash = parse_base16_or_base64(hash)
            .ok_or_else(|| format!("Hash is neither base16 nor base64"))?;

        let expected_bytes = alg.digest_bytes();
        let actual_bytes = expected_hash.len();
        if expected_bytes != actual_bytes {
            return Err(format!("Alg {alg} expected a digest of length {expected_bytes}, but got length {actual_bytes}"));
        }

        return Ok(HashArg::Hash {
            alg: vec![alg],
            expected_hash,
        });
    }

    if let Some(bytes) = parse_base16_or_base64(h) {
        let len = bytes.len();
        let alg = HashAlg::detect_from_length(len);
        if alg.is_empty() {
            return Err(format!("Could not detect hash algorithm from length {len}"));
        }

        return Ok(HashArg::Hash {
            alg: alg.into(),
            expected_hash: bytes,
        });
    }

    Err(
        "Provided argument is not a hash algorithm, SRI-style hash, nor is it base16 or base64"
            .into(),
    )
}

impl CompressionArg {
    /// Detect what compression format to use. If we couldn't figure it out,
    /// returns None.
    pub fn detect_format(&self, path: impl AsRef<Path>) -> Option<CompressionFormat> {
        match self {
            CompressionArg::Auto => {
                if let Some(ext) = path.as_ref().extension() {
                    Some(CompressionFormat::detect_from_extension(
                        &ext.to_string_lossy(),
                    ))
                } else {
                    None
                }
            }
            CompressionArg::None => Some(CompressionFormat::Identity),
            CompressionArg::Bz2 => Some(CompressionFormat::Bzip2),
            CompressionArg::Gz => Some(CompressionFormat::Gzip),
            CompressionArg::Xz => Some(CompressionFormat::Xz),
        }
    }
}

#[cfg(test)]
mod tests {
    use base64::Engine;

    use crate::hash::HashAlg;

    use super::{parse_hash_arg, HashArg};
    use test_case::test_case;

    #[test]
    fn parse_valid_sri_hash() {
        let result = parse_hash_arg(
            "sha384-EVSTQN3/azprG1Anm3QDgpJLIm9Nao0Yz1ztcQTwFspd3yD65VohhpuuCOmLASjC",
        )
        .unwrap();

        assert_eq!(
            result,
            HashArg::Hash {
                alg: vec![HashAlg::Sha384],
                expected_hash: base64::engine::general_purpose::STANDARD
                    .decode("EVSTQN3/azprG1Anm3QDgpJLIm9Nao0Yz1ztcQTwFspd3yD65VohhpuuCOmLASjC")
                    .unwrap()
            }
        )
    }

    #[test]
    fn parse_valid_sri_hash_base16() {
        let result = parse_hash_arg("md5-b7fbc56aaec74706d8fdae71aae7b0ac").unwrap();

        assert_eq!(
            result,
            HashArg::Hash {
                alg: vec![HashAlg::Md5],
                expected_hash: base16::decode("b7fbc56aaec74706d8fdae71aae7b0ac").unwrap()
            }
        )
    }

    #[test]
    fn parse_valid_base64_only_hash() {
        let result =
            parse_hash_arg("EVSTQN3/azprG1Anm3QDgpJLIm9Nao0Yz1ztcQTwFspd3yD65VohhpuuCOmLASjC")
                .unwrap();

        assert_eq!(
            result,
            HashArg::Hash {
                alg: vec![HashAlg::Sha384],
                expected_hash: base64::engine::general_purpose::STANDARD
                    .decode("EVSTQN3/azprG1Anm3QDgpJLIm9Nao0Yz1ztcQTwFspd3yD65VohhpuuCOmLASjC")
                    .unwrap()
            }
        )
    }

    #[test]
    fn parse_valid_base16_only_hash() {
        let result =
            parse_hash_arg("531a1557d205e09358e16fc4d79911ae4b9e28984bf10dbd7ab42d39f6a10713")
                .unwrap();

        assert_eq!(
            result,
            HashArg::Hash {
                alg: vec![HashAlg::Sha256],
                expected_hash: base16::decode(
                    "531a1557d205e09358e16fc4d79911ae4b9e28984bf10dbd7ab42d39f6a10713"
                )
                .unwrap()
            }
        );
    }

    #[test_case("skip")]
    #[test_case("none")]
    #[test_case("NONE"; "caps")]
    #[test_case("SkIp"; "mixed case")]
    fn parse_valid_skip(input: &str) {
        let result = parse_hash_arg(input).unwrap();

        assert_eq!(result, HashArg::Skip);
    }

    #[test_case("ask")]
    #[test_case("ASK"; "caps")]
    #[test_case("asK"; "mixed case")]
    fn parse_valid_ask(input: &str) {
        let result = parse_hash_arg(input).unwrap();

        assert_eq!(result, HashArg::Ask);
    }

    #[test_case("asdf-fdsu"; "bad algo")]
    #[test_case("sha256-vf798"; "bad length")]
    #[test_case("sha256-"; "no hash")]
    #[test_case(""; "empty")]
    #[test_case("f9od:fd"; "invalid chars")]
    fn parse_invalid_hash(input: &str) {
        parse_hash_arg(input).unwrap_err();
    }
}
