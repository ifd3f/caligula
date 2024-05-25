use run_mode::RunMode;

mod byteseries;
mod childproc_common;
mod compression;
mod device;
mod escalated_daemon;
mod escalation;
mod hash;
mod ipc_common;
mod logging;
mod native;
mod run_mode;
mod tty;
mod ui;
mod util;
mod writer_process;

pub mod bench {
    pub mod compression {
        pub use crate::compression::*;
    }
    pub mod writer_process_utils {
        pub use crate::writer_process::utils::*;
    }
}

#[inline(always)]
pub fn main() {
    match RunMode::detect() {
        RunMode::Main => ui::main::main(),
        RunMode::Writer => writer_process::main(),
        RunMode::EscalatedDaemon => escalated_daemon::main(),
    }
}
