use run_mode::RunMode;

mod byteseries;
mod compression;
mod device;
mod escalation;
mod hash;
mod ipc_common;
mod logging;
mod native;
mod run_mode;
mod ui;
mod writer_process;
mod escalated_daemon;

fn main() {
    match RunMode::detect() {
        RunMode::Main => ui::main::main(),
        RunMode::Writer => writer_process::child::main(),
        RunMode::EscalatedDaemon => escalated_daemon::main(),
    }
}
