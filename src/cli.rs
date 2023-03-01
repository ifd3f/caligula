use std::path::PathBuf;

use clap::{Parser, ValueEnum};

/// A safe ISO burner.
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Args {
    /// Input file to burn.
    #[arg(short)]
    pub input: PathBuf,

    /// Where to write the output. If not supplied, we will search for possible
    /// disks and ask you for where you want to burn.
    #[arg(short)]
    pub out: Option<PathBuf>,

    /// How to burn the input file.
    #[arg(short = 'm', long, value_enum, default_value_t = BurnMode::Standard)]
    pub burn_mode: BurnMode,

    /// If supplied, we will not ask for confirmation before destroying your disk.
    #[arg(short, long)]
    pub force: bool,

    /// Config to use, for advanced usages.
    #[arg(short, long)]
    pub config: Option<PathBuf>,

    /// Verbosity. Repeat to be more verbose.
    #[arg(short, action = clap::ArgAction::Count)]
    pub verbosity: u8,
}

#[derive(Debug, Default, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, ValueEnum)]
pub enum BurnMode {
    /// Normal mode.
    #[default]
    Standard,

    /// Treat the input as a Windows ISO.
    Win,
}
