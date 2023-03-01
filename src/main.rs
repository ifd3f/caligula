use clap::Parser;
use cli::Args;

pub mod cli;

fn main() {
    let args = Args::parse();
}
