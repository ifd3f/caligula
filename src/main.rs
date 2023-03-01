use crate::ui::ask_outfile;
use clap::Parser;
use cli::Args;

pub mod cli;
mod device;
mod ui;

fn main() {
    let _args = Args::parse();

    ask_outfile().unwrap();
}
