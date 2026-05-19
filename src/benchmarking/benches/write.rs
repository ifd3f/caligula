use std::{fs::File, path::PathBuf};

use clap::Parser;
use serde::{Deserialize, Serialize};

use crate::{
    benchmarking::{BenchContext, Benchmark, runner::BenchmarkParams},
    codec::compression::CompressionFormat,
    herder_api::write_verify::WVEvent,
    util::legacy_io::{WriteOp, open_blockdev},
};

/// Disk write benchmark.
#[derive(Parser, Debug, Serialize, Deserialize, Clone)]
pub struct WriteBench {
    /// Input file to write.
    #[arg(short, long, display_order = 0)]
    pub image: PathBuf,

    /// Disk to write to. THIS WILL BE ERASED WITHOUT CONFIRMATION!!!
    #[arg(short = 'o', long, display_order = 1)]
    // needs display_order = 1 or else it will go above image
    pub disk: PathBuf,

    /// What compression format the input file is in.
    #[arg(short = 'z', long, default_value = "identity")]
    pub compression: CompressionFormat,

    #[arg(long, default_value = "1048576")]
    pub file_read_buf_size: usize,

    #[arg(long, default_value = "1048576")]
    pub disk_write_buf_size: usize,

    #[arg(long, default_value = "4096")]
    pub disk_block_size: usize,
}

impl BenchmarkParams for WriteBench {
    type Report = ();

    fn setup(&self, ctx: &BenchContext) -> Box<dyn Benchmark<Report = Self::Report>> {
        let this = self.clone();

        let file = File::open(&this.image).expect("failed to open image");
        let disk = open_blockdev(&this.disk).expect("failed to open disk");
        ctx.set_progress_denominator(file.metadata().unwrap().len());

        Box::new(move |ctx: &BenchContext| {
            WriteOp {
                file,
                disk,
                cf: this.compression,
                buf_size: this.disk_write_buf_size,
                disk_block_size: this.disk_block_size,
                checkpoint_period: 32,
                file_read_buf_size: this.file_read_buf_size,
            }
            .execute(|e| {
                if let WVEvent::TotalBytes { src, .. } = e {
                    ctx.log_progress(src);
                }
            })
            .expect("operation failed");
        })
    }
}
