use run_mode::RunMode;

mod byteseries;
mod compression;
mod device;
mod escalated_daemon;
mod escalation;
mod hash;
mod logging;
mod native;
mod run_mode;
mod ui;
mod writer_process;
mod ipc_common;

fn main() {
    match RunMode::detect() {
        RunMode::Main => ui::main::main(),
        RunMode::Writer => writer_process::child::main(),
        RunMode::EscalatedDaemon => escalated_daemon::main(),
    }
}
