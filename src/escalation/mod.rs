#![allow(unused)]
#[cfg(target_os = "macos")]
mod darwin;
mod unix;

use std::process::Stdio;

pub use self::unix::Command;

#[derive(Debug, thiserror::Error)]
pub enum EscalationError {
    #[error("Failed to spawn process: {0}")]
    SpawnFailure(std::io::Error),

    #[error("Could not become root! Searched for sudo, doas, su")]
    UnixNotDetected,

    #[cfg(target_os = "macos")]
    #[error("User failed to confirm")]
    MacOSDenial,
}

pub async fn run_escalate(
    cmd: &Command<'_>,
    modify: impl FnOnce(&mut tokio::process::Command),
) -> Result<tokio::process::Child, EscalationError> {
    #[cfg(target_os = "linux")]
    {
        use self::unix::EscalationMethod;

        let mut cmd: tokio::process::Command = EscalationMethod::detect()?.wrap_command(cmd).into();
        modify(&mut cmd);
        Ok(cmd.spawn().map_err(EscalationError::SpawnFailure)?)
    }

    #[cfg(target_os = "macos")]
    {
        use self::darwin::wrap_osascript_escalation;

        wrap_osascript_escalation(cmd, modify).await
    }
}
