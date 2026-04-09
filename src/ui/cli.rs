use is_terminal::IsTerminal;
use itertools::Itertools;
use std::{fmt::Display, path::PathBuf};

use clap::{Parser, Subcommand, ValueEnum};

use crate::{
    compression::CompressionArg,
    hash::{HashAlg, parse_hash_input},
};

/// A lightweight, user-friendly disk imaging tool
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None, flatten_help = true)]
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
    /// Input image to burn.
    #[arg(value_parser = parse_image_path, display_order = 0)]
    pub image: PathBuf,

    /// Where to write the output. If not supplied, we will search for possible
    /// disks and ask you for where you want to burn.
    #[arg(short, display_order = 1)] // needs display_order = 1 or else it will go above image
    pub out: Option<PathBuf>,

    /// What compression format the input file is in.
    ///
    ///  - `auto` will guess based on the file extension.
    ///
    ///  - `ask` has the same behavior as `auto`, but with a confirmation.
    ///
    ///  - `none` means no compression.
    ///
    /// All other options are compression formats supported by this build of caligula.
    #[arg(short = 'z', long, default_value = "ask")]
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

    /// Where to look for the hash of the input file.
    #[arg(long, value_parser = parse_path_exists)]
    pub hash_file: Option<PathBuf>,

    /// Is the hash calculated from the raw file, or the compressed file?
    #[arg(long)]
    pub hash_of: Option<HashOf>,

    /// If provided, we will show all disks, removable or not.
    ///
    /// If you use this option, please proceed with caution!
    #[arg(long)]
    pub show_all_disks: bool,

    /// If we should run in interactive mode or not.
    ///
    /// Note that interactive mode will fail if all required arguments are not
    /// fully specified.
    #[arg(long, default_value = "auto")]
    pub interactive: Interactive,

    /// If supplied, we will not ask for confirmation before destroying your disk.
    #[arg(short, long)]
    pub force: bool,

    /// If we don't have permissions on the output file, should we try to become root?
    #[arg(long, default_value = "ask")]
    pub root: UseSudo,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Interactive {
    Auto,
    Always,
    Never,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum UseSudo {
    Ask,
    Always,
    Never,
}

fn parse_path_exists(p: &str) -> Result<PathBuf, String> {
    let path = PathBuf::from(p);
    if !path.exists() {
        return Err("path does not exist".to_string());
    }
    Ok(path)
}

fn parse_path_is_file(path: PathBuf) -> Result<PathBuf, String> {
    if !path.is_file() {
        return Err("path is not a file or symlink to a file".to_string());
    }
    Ok(path)
}

fn parse_image_path(p: &str) -> Result<PathBuf, String> {
    parse_path_exists(p).and_then(parse_path_is_file)
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

impl Interactive {
    pub fn is_interactive(&self) -> bool {
        match self {
            Interactive::Auto => std::io::stdin().is_terminal(),
            Interactive::Always => true,
            Interactive::Never => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use base64::Engine;

    use crate::hash::HashAlg;

    use super::{HashArg, parse_hash_arg};
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
