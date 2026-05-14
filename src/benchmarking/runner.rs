use std::{
    sync::atomic::{AtomicBool, AtomicU64, Ordering},
    time::Duration,
};

use indicatif::{ProgressBar, ProgressDrawTarget, ProgressStyle};
use serde::{Serialize, de::DeserializeOwned};

const REFRESH_PERIOD: Duration = Duration::from_millis(250);

pub fn run_benchmark<B: BenchmarkParams>(bench_params: B) {
    let ctx = BenchContext {
        progress: 0.into(),
        denominator: 0.into(),
        finished: false.into(),
    };
    let ctx = &ctx;

    std::thread::scope(move |s| {
        // set up the bench
        let bench = bench_params.setup(ctx);

        // spawn progress bar thread
        let jh = std::thread::Builder::new()
            .name("benchbar".into())
            .spawn_scoped(s, || progress_bar_thread(ctx))
            .unwrap();

        // run bench in this thread
        bench.run(ctx);

        // now that we're done, notify and wake up the progress bar thread so we finish
        // asap
        ctx.finished.store(true, Ordering::SeqCst);
        jh.thread().unpark();
    });
}

/// Interface for receiving data from benchmarks.
pub struct BenchContext {
    progress: AtomicU64,
    denominator: AtomicU64,

    /// Flag for whether or not this is finished. It's mainly used to signal
    /// termination to the [`progress_bar_thread()`].
    finished: AtomicBool,
}

impl BenchContext {
    /// Set the denominator to use for progress tracking.
    pub fn set_progress_denominator(&self, denominator: u64) {
        self.denominator.store(denominator, Ordering::Relaxed);
    }

    /// Log progress. This can be any arbitrary unit, but in order for it to
    /// take effect, [`Self::set_progress_denominator()`] must have been
    /// called with a non-zero value.
    pub fn log_progress(&self, progress: u64) {
        self.progress.store(progress, Ordering::Relaxed);
    }
}

/// Canned arguments for creating benchmarks.
pub trait BenchmarkParams: Clone + Sync + Serialize + DeserializeOwned + 'static {
    /// Additional data to report from this benchmark.
    type Report: Serialize + DeserializeOwned + Send + 'static;

    /// Prepare a benchmark to be executed.
    fn setup(&self, ctx: &BenchContext) -> Box<dyn Benchmark<Report = Self::Report>>;
}

/// A benchmark that has been fully set up, and is ready to be executed.
pub trait Benchmark: Send + 'static {
    /// Additional data to report from this benchmark.
    type Report: Serialize + DeserializeOwned + Send + 'static;

    /// Execute the benchmark.
    fn run(self: Box<Self>, ctx: &BenchContext) -> Self::Report;
}

impl<F, R> Benchmark for F
where
    F: FnOnce(&BenchContext) -> R + Send + 'static,
    R: Serialize + DeserializeOwned + Send + 'static,
{
    type Report = R;

    fn run(self: Box<Self>, ctx: &BenchContext) -> Self::Report {
        self(ctx)
    }
}

/// renders a progress bar!
fn progress_bar_thread(ctx: &BenchContext) {
    // set it up
    let len = 80;
    let bar = ProgressBar::new(len)
        .with_message("Benching")
        .with_style(make_progress_style(false));
    bar.set_draw_target(ProgressDrawTarget::stderr());

    while !ctx.finished.load(Ordering::SeqCst) {
        let denominator = ctx.denominator.load(Ordering::Relaxed);
        let progress = ctx.progress.load(Ordering::Relaxed);
        if denominator != 0 {
            bar.set_position((progress as f64 * len as f64 / denominator as f64) as u64);
        }

        // use park timeout instead of sleep. this way we can wake me up inside
        // as soon as the actual bench finishes.
        std::thread::park_timeout(REFRESH_PERIOD);
    }

    // set to 100% progress
    bar.set_style(make_progress_style(true));
    bar.set_position(len);
    bar.finish_with_message("Done!");
}

fn make_progress_style(is_done: bool) -> ProgressStyle {
    const IN_PROGRESS: &str = "[{elapsed_precise}] {wide_bar:.yellow} {percent:>3}%";
    const DONE: &str = "[{elapsed_precise}] {wide_bar:.green} {percent:>3}%";

    let template = match is_done {
        true => DONE,
        false => IN_PROGRESS,
    };

    ProgressStyle::with_template(template).unwrap()
}
