use itertools::Itertools;
use std::{fmt::Display, path::PathBuf};

use clap::{Parser, Subcommand, ValueEnum};

use crate::{
    compression::CompressionArg,
    hash::{parse_hash_input, HashAlg},
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

    /// What compression format the input file is. If `auto`, then we will guess
    /// based on the extension.
    #[arg(short = 'z', long, default_value = "auto")]
    pub compression: CompressionArg,

    /// The hash of the input file. This can be provided in one of several formats:
    ///
    ///  - `ask` to ask the user for a hash
    ///
    ///  - `skip` or `none` to not do hash verification
    ///
    ///  - an SRI-like string with either base16 or base64 in the format of `<alg>-<hash>`
    ///    (i.e. `sha256-EVSTQN3/azprGF...`)
    ///
    ///  - just a hash value, and we will guess the algorithm (i.e. `EVSTQN3/azprGF...`)
    ///
    /// The following algorithms are supported: md5, sha1, sha224, sha256, sha384, sha512
    #[arg(
        short = 's',
        long,
        value_parser = parse_hash_arg,
        default_value = "ask",
        help = "The hash of the input file. For more information, see long help (--help)"
    )]
    pub hash: HashArg,

    /// Is the hash calculated from the raw file, or the compressed file?
    #[arg(long)]
    pub hash_of: Option<HashOf>,

    /// If provided, we will show all disks, removable or not.
    ///
    /// If you use this option, please proceed with caution!
    #[arg(long)]
    pub show_all_disks: bool,

    /// If supplied, we will not ask for confirmation before destroying your disk.
    #[arg(short, long)]
    pub force: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HashArg {
    Ask,
    Skip,
    Hash {
        alg: HashAlg,
        expected_hash: Vec<u8>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum HashOf {
    Raw,
    Compressed,
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
        "ask" => Ok(HashArg::Ask),
        "skip" | "none" => Ok(HashArg::Skip),
        _ => match parse_hash_input(h) {
            Ok((alg, expected_hash)) => {
                if alg.len() > 1 {
                    Err(format!(
                        "Ambiguous hash algorithm! Could be one of: {}. Please specify by prepending [alg]- to your hash.",
                        alg.iter().format(", ")
                    ))
                } else {
                    Ok(HashArg::Hash {
                        alg: alg[0],
                        expected_hash,
                    })
                }
            }
            Err(e) => Err(format!("{e}")),
        },
    }
}

impl Display for HashOf {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HashOf::Raw => write!(f, "raw"),
            HashOf::Compressed => write!(f, "compressed"),
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
    fn parse_valid_hash() {
        let result = parse_hash_arg(
            "sha384-EVSTQN3/azprG1Anm3QDgpJLIm9Nao0Yz1ztcQTwFspd3yD65VohhpuuCOmLASjC",
        )
        .unwrap();

        assert_eq!(
            result,
            HashArg::Hash {
                alg: HashAlg::Sha384,
                expected_hash: base64::engine::general_purpose::STANDARD
                    .decode("EVSTQN3/azprG1Anm3QDgpJLIm9Nao0Yz1ztcQTwFspd3yD65VohhpuuCOmLASjC")
                    .unwrap()
            }
        )
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
