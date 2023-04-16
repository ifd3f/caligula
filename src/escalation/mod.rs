#[cfg(target_os = "macos")]
mod darwin;
mod unix;

use std::process::Command;
use tokio::process::Command as AsyncCommand;

#[derive(Debug, thiserror::Error)]
#[allow(dead_code)]
pub enum Error {
    #[error("Could not become root! Searched for sudo, doas, su")]
    UnixNotDetected,
    #[error("User failed to confirm")]
    MacOSDenial,
}

#[cfg(target_os = "linux")]
pub async fn run_escalate(cmd: Command) -> anyhow::Result<tokio::process::Child> {
    use self::unix::EscalationMethod;

    let mut cmd: AsyncCommand = EscalationMethod::detect()?.wrap_command(cmd).into();
    cmd.kill_on_drop(true);
    Ok(cmd.spawn()?)
}

#[cfg(target_os = "macos")]
pub fn run_escalate(cmd: Command) -> anyhow::Result<tokio::process::Child> {
    use self::darwin::wrap_osascript_escalation;

    wrap_osascript_escalation(cmd)
}
