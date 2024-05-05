use std::env;

use serde::de::DeserializeOwned;
use std::fmt::Debug;
use tracing::{error, info};
use tracing_unwrap::ResultExt;
use valuable::Valuable;

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
pub fn child_init<C: DeserializeOwned + Debug + Valuable>() -> (String, C) {
    let cli_args: Vec<String> = env::args().collect();

    // We will set up logging first because if any part of this godforsaken
    // process fails, at the very least we'll have logging :)
    let log_file = &cli_args[1];
    init_logging_child(log_file);
    std::panic::set_hook(Box::new(|p| {
        error!("{p:#?}");
    }));

    let socket_path = &cli_args[2];
    let args = serde_json::from_str::<C>(&cli_args[3]).unwrap_or_log();
    info!(
        ?socket_path,
        args = args.as_value(),
        "We are in child process mode"
    );

    (socket_path.clone(), args)
}
