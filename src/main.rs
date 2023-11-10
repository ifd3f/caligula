use run_mode::RunMode;

mod byteseries;
mod compression;
mod device;
mod escalation;
mod hash;
mod logging;
mod native;
mod run_mode;
mod ui;
mod writer_process;

fn main() {
    match RunMode::detect() {
        RunMode::Main => ui::main::main(),
        RunMode::Writer => writer_process::child::main(),
    }
}
