use std::path::PathBuf;

use clap::{Parser, ValueEnum};
use serde::{Deserialize, Serialize};
use valuable::Valuable;

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

    /// How to burn the input file. If not supplied, it will be detected from
    /// the provided ISO.
    #[arg(short = 'm', long, value_enum)]
    pub burn_mode: Option<BurnMode>,

    /// If supplied, we will not ask for confirmation before destroying your disk.
    #[arg(short, long)]
    pub force: bool,

    /// If provided, we will not only show you removable disks, but all disks.
    /// If you use this option, please proceed with caution!
    #[arg(long)]
    pub show_all_disks: bool,

    /// Config to use, for advanced usages.
    #[arg(short, long)]
    pub config: Option<PathBuf>,

    /// Verbosity. Repeat to be more verbose.
    #[arg(short, action = clap::ArgAction::Count)]
    pub verbosity: u8,
}

#[derive(
    Debug,
    Default,
    Copy,
    Clone,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    ValueEnum,
    Deserialize,
    Serialize,
    Valuable,
)]
pub enum BurnMode {
    /// Normal mode.
    #[default]
    Normal,

    /// Treat the input as a Windows ISO.
    Win,
}
