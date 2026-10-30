use std::{fs::File, path::PathBuf, time::Duration};

use clap::Parser;
use serde::{Deserialize, Serialize};

use crate::{
    benchmarking::{BenchContext, Benchmark, runner::BenchmarkParams},
    codec::{compression::CompressionFormat, hash::HashAlg},
    facade::workflow::{self, WorkflowState, hash::HashWorkflow},
};

/// File read and hash calculation benchmark.
#[derive(Parser, Debug, Serialize, Deserialize, Clone)]
pub struct HashBenchParams {
    /// Input image to hash.
    #[arg(display_order = 0)]
    pub input: PathBuf,

    /// What hash algorithm to run over the input.
    #[arg(short = 'a', long)]
    pub alg: HashAlg,

    /// What compression format the input file is in.
    #[arg(short = 'z', long, default_value = "identity")]
    pub compression: CompressionFormat,
}

impl BenchmarkParams for HashBenchParams {
    type Report = ();

    fn setup(&self, ctx: &BenchContext) -> Box<dyn Benchmark<Report = Self::Report>> {
        let this = self.clone();

        let file = File::open(&this.input).unwrap();
        let size = file.metadata().unwrap().len();
        ctx.set_progress_denominator(size);

        Box::new(move |ctx: &BenchContext| {
            let rt = tokio::runtime::LocalRuntime::new().unwrap();
            rt.block_on(async move {
                let w = workflow::hash::run(&HashWorkflow {
                    file: this.input,
                    alg: this.alg,
                    compression: this.compression,
                });

                let mut interval = tokio::time::interval(Duration::from_secs(1));
                let r = loop {
                    interval.tick().await;

                    let state = w.borrow();

                    ctx.log_progress(state.read_bytes_history().bytes_encountered());

                    if let Some(r) = state.result().cloned() {
                        break r;
                    }
                };

                println!("{:x?}", r.expect("hashing unexpectedly failed").to_vec());
            });
        })
    }
}
