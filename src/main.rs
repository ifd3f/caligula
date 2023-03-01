use crate::ui::ask_outfile;
use clap::Parser;
use cli::Args;
use ui::fopen::open_or_escalate;

pub mod burn;
pub mod cli;
mod device;
mod ui;

fn main() {
    let args = Args::parse();

    let target = ask_outfile(&args).unwrap();
    let in_file = open_or_escalate(target.devnode).unwrap();
    let out_dev = open_or_escalate(args.input).unwrap();
}
