use std::{borrow::Cow, fmt::Debug};

use process_path::get_executable_path;
use serde::Serialize;
use valuable::Valuable;

use crate::{
    escalated_daemon::ipc::EscalatedDaemonInitConfig, escalation::Command,
    writer_process::ipc::WriterProcessConfig,
};

pub const RUN_MODE_ENV_NAME: &str = "__CALIGULA_RUN_MODE";

/// [RunMode] is a flag set in the environment variable `__CALIGULA_RUN_MODE`
/// to signal which process we are.
///
/// # Motivation
///
/// What we would ideally like to do is write code that looks like this, to
/// escalate privileges:
///
/// ```
/// let is_child_process = fork();
///
/// if is_child_process {
///     if need_to_sudo {
///         become_root();
///     }
///     run_child_procedure();
/// } else {
///     run_parent_procedure();
/// }
/// ```
///
/// Unfortunately, the best we can do is call `sudo`/`doas`/`su` on ourself,
/// and detect if we are the child process.
///
/// Thus, this "private" environment variable and detection code is here to
/// help us achieve that.
///
/// *Why is this an environment variable instead of a CLI subcommand?* It's
/// meant to be hidden from the user as much as possible. There is no reason
/// that the user should ever set the run mode.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum RunMode {
    Main,
    Writer,
    EscalatedDaemon,
}

impl RunMode {
    pub fn detect() -> Self {
        match std::env::var(RUN_MODE_ENV_NAME).as_deref() {
            Ok("writer") => Self::Writer,
            Ok("escalated_daemon") => Self::EscalatedDaemon,
            _ => Self::Main,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            RunMode::Main => "main",
            RunMode::Writer => "writer",
            RunMode::EscalatedDaemon => "escalated_daemon",
        }
    }
}

/// Build a [Command] that, when run, spawns a process with a specific configuration.
pub fn make_spawn_command<'a, C: Serialize + Debug + Valuable>(
    socket: Cow<'a, str>,
    log_path: Cow<'a, str>,
    run_mode: RunMode,
    init_config: C,
) -> Command<'a> {
    let proc = get_executable_path().unwrap();

    Command {
        proc: proc.to_str().unwrap().to_owned().into(),
        envs: vec![(RUN_MODE_ENV_NAME.into(), run_mode.as_str().into())],
        // Arg order is documented in childproc_common.
        args: vec![
            log_path.into(),
            socket.into(),
            serde_json::to_string(&init_config).unwrap().into(),
        ],
    }
}

pub fn make_writer_spawn_command<'a>(
    socket: Cow<'a, str>,
    log_path: Cow<'a, str>,
    init_config: &WriterProcessConfig,
) -> Command<'a> {
    make_spawn_command(socket, log_path, RunMode::Writer, init_config)
}

pub fn make_escalated_daemon_spawn_command<'a>(
    socket: Cow<'a, str>,
    log_path: Cow<'a, str>,
    init_config: &EscalatedDaemonInitConfig,
) -> Command<'a> {
    make_spawn_command(socket, log_path, RunMode::EscalatedDaemon, init_config)
}
