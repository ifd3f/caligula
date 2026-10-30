mod runner;
mod benches {
    pub mod hash;
    pub mod verify;
    pub mod write;
}

use std::path::PathBuf;

use clap::{Parser, Subcommand};

pub use self::runner::BenchContext;
use crate::benchmarking::runner::run_benchmark;

const BENCH_ABOUT: &str = "Caligula benchmarking subsystem.

All benchmarks assume you have adequate permissions to read and write the files you pass in. No \
                           auto-escalation is done.

WARNING: THESE COMMANDS ARE EXTREMELY DANGEROUS! THIS IS A SUBSYSTEM MEANT FOR CALIGULA \
                           DEVELOPERS, RATHER THAN END USERS! COMMAND STABILITY IS NOT \
                           GUARANTEED, AND IF YOU USE THIS, DO NOT EXPECT TO RECEIVE ANY HELP \
                           WHATSOEVER! YOU HAVE BEEN WARNED!";

#[derive(Parser, Debug)]
#[clap(about = BENCH_ABOUT)]
pub struct BenchArgs {
    #[command(name = "type", subcommand)]
    pub type_: BenchType,
}

#[derive(Subcommand, Debug)]
pub enum BenchSubcommands {
    Run(RunBenchArgs),
}

/// Generate a report from one or multiple benchmark runs.
#[derive(Parser, Debug)]
struct ReportBenchArgs {
    /// Files to read from. If not provided, reads from stdin.
    pub result_files: Vec<PathBuf>,

    /// Tags to consider as the "base" benchmark, or empty to not work in
    /// comparison mode. Any runs not having this tag will be considered the
    /// "comparison."
    #[arg(short, long, value_delimiter = ',')]
    pub base: Vec<String>,
}

#[derive(Parser, Debug)]
struct RunBenchArgs {
    /// Which benchmark to run.
    #[command(name = "type", subcommand)]
    pub type_: BenchType,
}

#[derive(Subcommand, Debug)]
enum BenchType {
    Write(benches::write::WriteBench),
    Hash(benches::hash::HashBenchParams),
    Verify(benches::verify::VerifyBench),
}

pub fn main(args: BenchArgs) {
    match args.type_ {
        BenchType::Write(b) => run_benchmark(b),
        BenchType::Hash(b) => run_benchmark(b),
        BenchType::Verify(b) => run_benchmark(b),
    }
}
