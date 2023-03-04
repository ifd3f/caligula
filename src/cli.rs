use std::path::PathBuf;

use clap::{Parser, Subcommand};

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
}

fn parse_path_exists(p: &str) -> Result<PathBuf, String> {
    let path = PathBuf::from(p);
    if !path.exists() {
        return Err(format!("path does not exist"));
    }
    Ok(path)
}
