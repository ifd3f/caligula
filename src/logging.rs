use std::fs::File;
use std::panic::set_hook;
use std::path::Path;
use std::sync::Mutex;

use crossterm::terminal::disable_raw_mode;
use tracing::Level;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::fmt::format::FmtSpan;

/// Helper for calculating which files to log to.
#[derive(Debug, Clone)]
pub struct LogPaths {
    log_path: String,
}

impl LogPaths {
    pub fn init(state_dir: impl AsRef<Path>) -> Self {
        Self {
            log_path: if cfg!(debug_assertions) {
                "caligula.log".into()
            } else {
                state_dir
                    .as_ref()
                    .join("caligula.log")
                    .to_str()
                    .unwrap()
                    .to_owned()
            },
        }
    }

    pub fn main(&self) -> &str {
        &self.log_path
    }

    pub fn get_bug_report_msg(&self) -> String {
        format!(
            "Please report bugs to https://github.com/ifd3f/caligula/issues and attach the \
        log files in {}",
            self.log_path
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
        tracing_panic::panic_hook(p);

        disable_raw_mode().ok();

        eprintln!("An unexpected error occurred: {p}");
        eprintln!();
        eprintln!("{}", bug_report_msg);
    }));

    let file = File::create(paths.main()).unwrap();

    init_tracing_subscriber(file);
}

pub fn init_logging_child(write_path: impl AsRef<Path>) {
    let file = File::options().append(true).open(write_path).unwrap();
    init_tracing_subscriber(file);
    set_hook(Box::new(tracing_panic::panic_hook));
}

fn init_tracing_subscriber(file: File) {
    tracing_subscriber::fmt()
        .compact()
        .with_ansi(false) // hide colors
        .with_writer(Mutex::new(file))
        .with_span_events(FmtSpan::FULL)
        .with_env_filter(
            EnvFilter::builder()
                .with_default_directive(FILE_LOG_LEVEL.into())
                .from_env_lossy(),
        )
        .with_thread_names(true)
        .with_file(true)
        .with_line_number(true)
        .init();
}
