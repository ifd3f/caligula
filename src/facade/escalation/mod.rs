#![allow(unused)]
mod unix;

use std::process::Stdio;

pub use self::unix::{Command, EscalationMethod};

#[derive(Debug, thiserror::Error)]
pub enum EscalationError {
    #[error("Failed to spawn process: {0}")]
    SpawnFailure(std::io::Error),

    #[error("Could not become root! Searched for sudo, doas, su")]
    UnixNotDetected,
}

pub async fn run_escalate(
    cmd: &Command<'_>,
    modify: impl FnOnce(&mut tokio::process::Command),
) -> Result<tokio::process::Child, EscalationError> {
    let mut cmd: tokio::process::Command = EscalationMethod::detect()?.wrap_command(cmd).into();
    modify(&mut cmd);
    cmd.spawn().map_err(EscalationError::SpawnFailure)
}
