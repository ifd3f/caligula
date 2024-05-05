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

    pub fn as_str(&self) -> &str {
        match self {
            RunMode::Main => "main",
            RunMode::Writer => "writer",
            RunMode::EscalatedDaemon => "escalated_daemon",
        }
    }
}
