use clap::Parser;
use cli::Args;
use outfile::ask_outfile;

pub mod cli;
pub mod outfile;

fn main() {
    let _args = Args::parse();

    ask_outfile().unwrap();
}
