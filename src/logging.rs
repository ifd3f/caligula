use std::fs::File;
use std::fs::create_dir_all;
use std::panic::set_hook;
use std::path::Path;
use std::{path::PathBuf, sync::Mutex};

use crossterm::terminal::disable_raw_mode;
use tracing::{Level, error};
use tracing_subscriber::EnvFilter;

/// Helper for calculating which files to log to.
#[derive(Debug, Clone)]
pub struct LogPaths {
    log_dir: PathBuf,
}

impl LogPaths {
    pub fn init(state_dir: impl AsRef<Path>) -> Self {
        let log_dir = if cfg!(debug_assertions) {
            PathBuf::from("dev")
        } else {
            state_dir.as_ref().join("log")
        };
        create_dir_all(&log_dir).unwrap();
        Self { log_dir }
    }

    pub fn main(&self) -> PathBuf {
        self.log_dir.join("main.log")
    }

    pub fn escalated_daemon(&self) -> PathBuf {
        self.log_dir.join("escalated.log")
    }

    pub fn writer(&self, id: u64) -> PathBuf {
        self.log_dir.join(format!("writer-{id}.log"))
    }

    pub fn get_bug_report_msg(&self) -> String {
        format!(
            "Please report bugs to https://github.com/ifd3f/caligula/issues and attach the \
        log files in {}",
            self.log_dir.to_string_lossy()
        )
    }
}

#[cfg(not(debug_assertions))]
const FILE_LOG_LEVEL: Level = Level::DEBUG;

#[cfg(debug_assertions)]
const FILE_LOG_LEVEL: Level = Level::TRACE;

pub fn init_logging_parent(paths: &LogPaths) {
    let bug_report_msg = paths.get_bug_report_msg();
    set_hook(Box::new(move |p| {
        disable_raw_mode().ok();
        error!("{p}");

        eprintln!("An unexpected error occurred: {p}");
        eprintln!();
        eprintln!("{}", bug_report_msg);
    }));

    let write_path = paths.main();

    init_tracing_subscriber(write_path);
}

pub fn init_logging_child(write_path: impl AsRef<Path>) {
    init_tracing_subscriber(write_path);
}

fn init_tracing_subscriber(write_path: impl AsRef<Path>) {
    let writer = File::create(write_path).unwrap();

    tracing_subscriber::fmt()
        .compact()
        .with_ansi(false)
        .with_writer(Mutex::new(writer))
        .with_env_filter(
            EnvFilter::builder()
                .with_default_directive(FILE_LOG_LEVEL.into())
                .from_env_lossy(),
        )
        .with_file(true)
        .with_line_number(true)
        .init();
}
