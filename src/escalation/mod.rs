#![allow(unused)]
#[cfg(target_os = "macos")]
mod darwin;
mod unix;

use std::process::Stdio;

pub use self::unix::Command;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Could not become root! Searched for sudo, doas, su")]
    UnixNotDetected,

    #[cfg(target_os = "macos")]
    #[error("User failed to confirm")]
    MacOSDenial,
}

pub async fn run_escalate(
    cmd: &Command<'_>,
    modify: impl FnOnce(&mut tokio::process::Command) -> (),
) -> anyhow::Result<tokio::process::Child> {
    #[cfg(target_os = "linux")]
    {
        use self::unix::EscalationMethod;

        let mut cmd: tokio::process::Command = EscalationMethod::detect()?.wrap_command(cmd).into();
        modify(&mut cmd);
        cmd.stdin(Stdio::null());
        Ok(cmd.spawn()?)
    }

    #[cfg(target_os = "macos")]
    {
        use self::darwin::wrap_osascript_escalation;

        wrap_osascript_escalation(cmd, modify).await
    }
}
