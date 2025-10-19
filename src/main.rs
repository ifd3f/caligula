use run_mode::RunMode;

mod byteseries;
mod childproc_common;
mod compression;
mod device;
mod escalated_daemon;
mod escalation;
mod hash;
mod hashfile;
mod herding;
mod ipc_common;
mod logging;
mod native;
mod run_mode;
mod tty;
mod ui;
mod util;
mod writer_process;
mod writer;

fn main() {
    match RunMode::detect() {
        RunMode::Main => ui::main::main(),
        RunMode::Writer => writer_process::main(),
        RunMode::EscalatedDaemon => escalated_daemon::main(),
    }
}
