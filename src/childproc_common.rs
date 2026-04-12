use std::env;

use serde::de::DeserializeOwned;
use std::fmt::Debug;
use tracing::{error, info};
use tracing_unwrap::ResultExt;

use crate::logging::init_logging_child;

/// Initialize this process as a generic child process. The following actions
/// are performed:
///
/// - Get the logging file from arg 1 and:
///     - initialize logging
///     - set up a panic hook
/// - Get socket path from arg 2
/// - Get the child-specific config from arg 3
///
/// This returns the socket path and the child-specific config.
pub fn child_init<C: DeserializeOwned + Debug>(log_file: &str) -> C {
    let cli_args: Vec<String> = env::args().collect();

    init_logging_child(log_file);
    std::panic::set_hook(Box::new(|p| {
        error!("{p:#?}");
    }));

    let args = serde_json::from_str::<C>(&cli_args[2]).unwrap_or_log();
    info!(?args, "We are in child process mode");

    args
}
