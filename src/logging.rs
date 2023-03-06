use std::fs::File;
use std::panic::set_hook;
use std::path::Path;
use std::{env, fs::create_dir_all, time::SystemTime};
use std::{path::PathBuf, sync::Mutex};

use static_cell::StaticCell;
use tracing::{Level, error};

#[derive(Debug, Clone)]
pub struct LogPaths {
    pub main: PathBuf,
    pub child: PathBuf,
}

static LOG_PATHS: StaticCell<LogPaths> = StaticCell::new();
static mut LOG_PATHS_REF: Option<&'static LogPaths> = None;

pub const CHILD_LOG_PATH_ENV: &str = "_CALIGULA_CHILD_LOG_PATH";

#[cfg(not(debug_assertions))]
const FILE_LOG_LEVEL: Level = Level::DEBUG;

#[cfg(debug_assertions)]
const FILE_LOG_LEVEL: Level = Level::TRACE;

pub fn init_logging_parent() {
    init_log_paths();

    set_hook(Box::new(|p| {
        error!("{p}");

        let paths = get_log_paths();
        eprintln!("An unexpected error occurred! Please report bugs to https://github.com/ifd3f/caligula/issues and attach the following files, if they exist:");
        eprintln!(" - {}", paths.main.to_string_lossy());
        eprintln!(" - {}", paths.child.to_string_lossy());
        eprintln!("");
        eprintln!("{}", p);
    }));

    let write_path = get_log_paths().main.clone();

    let writer = File::create(write_path).unwrap();

    tracing_subscriber::fmt()
        .with_writer(Mutex::new(writer))
        .with_max_level(FILE_LOG_LEVEL)
        .init();
}

pub fn init_logging_child(write_path: impl AsRef<Path>) {
    let writer = File::create(write_path).unwrap();

    tracing_subscriber::fmt()
        .with_writer(Mutex::new(writer))
        .with_max_level(FILE_LOG_LEVEL)
        .init();
}

pub fn get_log_paths() -> &'static LogPaths {
    unsafe { LOG_PATHS_REF.expect("Logging has not been initialized") }
}

fn init_log_paths() {
    let log_prefix = env::temp_dir().join(format!(
        "caligula/log/{}",
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_millis()
    ));

    create_dir_all(log_prefix.parent().unwrap()).unwrap();

    let pref = LOG_PATHS.init(LogPaths {
        main: log_prefix.with_extension("main.log").into(),
        child: log_prefix.with_extension("child.log").into(),
    });

    unsafe {
        // This is safe because we are the only writer, and we
        // should be writing before anyone else reads it
        LOG_PATHS_REF = Some(pref);
    }
}
