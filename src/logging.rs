use std::fs::File;
use std::panic::set_hook;
use std::path::Path;
use std::{env, fs::create_dir_all, time::SystemTime};
use std::{path::PathBuf, sync::Mutex};

use crossterm::terminal::disable_raw_mode;
use static_cell::StaticCell;
use tracing::{error, Level};
use tracing_subscriber::EnvFilter;

#[derive(Debug, Clone)]
pub struct LogPaths {
    log_dir: PathBuf,
}

impl LogPaths {
    pub fn main(&self) -> PathBuf {
        self.log_dir.join("main.log")
    }

    pub fn writer(&self, id: u64) -> PathBuf {
        self.log_dir.join(format!("writer-{id}.log"))
    }

    pub fn log_dir(&self) -> &PathBuf {
        &self.log_dir
    }
}

static LOG_PATHS: StaticCell<LogPaths> = StaticCell::new();
static mut LOG_PATHS_REF: Option<&'static LogPaths> = None;

#[cfg(not(debug_assertions))]
const FILE_LOG_LEVEL: Level = Level::DEBUG;

#[cfg(debug_assertions)]
const FILE_LOG_LEVEL: Level = Level::TRACE;

pub fn init_logging_parent() {
    init_log_paths();

    set_hook(Box::new(|p| {
        disable_raw_mode().ok();
        error!("{p}");

        eprintln!("An unexpected error occurred: {p}");
        eprintln!();
        eprintln!("{}", get_bug_report_msg());
    }));

    let write_path = get_log_paths().main().clone();

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
        .init();
}

pub fn get_log_paths() -> &'static LogPaths {
    unsafe { LOG_PATHS_REF.expect("Logging has not been initialized") }
}

fn init_log_paths() {
    let log_dir = if cfg!(debug_assertions) {
        PathBuf::from("dev")
    } else {
        env::temp_dir().join(format!(
            "caligula/log/{}",
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_millis()
        ))
    };

    create_dir_all(&log_dir).unwrap();

    let pref = LOG_PATHS.init(LogPaths { log_dir });

    unsafe {
        // This is safe because we are the only writer, and we
        // should be writing before anyone else reads it
        LOG_PATHS_REF = Some(pref);
    }
}

pub fn get_bug_report_msg() -> String {
    let paths = get_log_paths();

    format!(
        "Please report bugs to https://github.com/ifd3f/caligula/issues and attach the \
        log files in {}",
        paths.log_dir().to_string_lossy()
    )
}
