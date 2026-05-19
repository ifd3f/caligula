//! Helpers for logging and crash reporting.

mod error;
mod sysinfo;

use std::{
    self,
    fs::File,
    panic::{PanicHookInfo, set_hook},
    path::Path,
    sync::{Arc, Mutex},
};

pub use error::{ErrorContext, ErrorInfo, ErrorSeverity, ErrorWithInfo, crash_and_burn};
use tracing::Level;
use tracing_subscriber::{EnvFilter, fmt::format::FmtSpan};

use crate::logging::error::RemediationAdvice;

const ISSUE_TRACKER_URL: &str = "https://github.com/ifd3f/caligula/issues";

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
            "This is likely a bug caused by developer error. Please report the issue to https://github.com/ifd3f/caligula/issues and attach the log file in {}",
            self.log_path
        )
    }
}

#[cfg(not(debug_assertions))]
const FILE_LOG_LEVEL: Level = Level::DEBUG;

#[cfg(debug_assertions)]
const FILE_LOG_LEVEL: Level = Level::TRACE;

pub fn init_logging_parent(paths: &LogPaths) -> Arc<error::ErrorContext> {
    let log_path = paths.main().to_owned();
    let file = File::create(&log_path).expect("Failed to create log file!");

    init_tracing_subscriber(file);

    sysinfo::log_program_info();
    sysinfo::log_uname();
    sysinfo::log_info_files();
    sysinfo::log_environment_variables();

    let error_context = Arc::new(error::ErrorContext { log_path });

    let ctx = error_context.clone();

    set_hook(Box::new(move |p| {
        tracing_panic::panic_hook(p);

        crash_and_burn(&ctx, PanicError(p));
    }));

    error_context
}

/// Simple wrapper for panic info that implements [`ErrorWithInfo`].
#[derive(Debug, thiserror::Error)]
#[error("{0}")]
struct PanicError<'a>(&'a PanicHookInfo<'a>);

impl<'a> ErrorWithInfo for PanicError<'a> {
    fn error_info(&self) -> ErrorInfo {
        ErrorInfo {
            severity: ErrorSeverity::Panic,
            remediation: RemediationAdvice::DeveloperError,
        }
    }
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
        .with_span_events(FmtSpan::NEW | FmtSpan::CLOSE)
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
