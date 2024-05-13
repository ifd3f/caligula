#![allow(unused)]
#[cfg(target_os = "macos")]
mod darwin;
mod hidden_input;
mod unix;

use std::{os::fd::AsRawFd, process::Stdio};

use tokio::io::{
    AsyncBufRead, AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader,
};
use tracing::{debug, debug_span, info, Instrument};

use crate::escalation::hidden_input::HiddenInput;

pub use self::unix::Command;

/// A token that is used to detect if a process has been escalated.
///
/// If the child process spits this out on stdout that means we escalated successfully.
pub const SUCCESS_TOKEN: &'static str = "znxbnvm,,xbnzcvnmxzv,.,,,";

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Could not become root! Searched for sudo, doas, su")]
    UnixNotDetected,

    #[cfg(target_os = "macos")]
    #[error("User failed to confirm")]
    MacOSDenial,
}

#[tracing::instrument(skip_all)]
pub async fn run_escalate(
    cmd: &Command<'_>,
    modify: impl FnOnce(&mut tokio::process::Command) -> (),
) -> anyhow::Result<tokio::process::Child> {
    use self::unix::EscalationMethod;

    let mut cmd: tokio::process::Command = EscalationMethod::detect()?.wrap_command(cmd).into();
    modify(&mut cmd);
    // inherit sucks but it's unfortunately the best way to do this for now.
    cmd.stdin(Stdio::inherit())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    info!(?cmd, "Spawning child process");
    let mut proc = cmd.spawn()?;
    let mut stdout = BufReader::new(proc.stdout.take().unwrap());
    let mut stderr = proc.stderr.take().unwrap();

    let tty = std::fs::File::open("/dev/tty")?;
    let _hidden = HiddenInput::new(tty.as_raw_fd())?;

    // Search for the success token
    match event_loop(tokio::io::stderr(), stdout, stderr).await? {
        true => Ok(proc),
        false => anyhow::bail!("Could not escalate"),
    }
}

#[tracing::instrument(skip_all, level = "trace")]
async fn event_loop(
    mut parent_stderr: impl AsyncWrite + Unpin,
    mut child_stdout: impl AsyncBufRead + Unpin + Send + 'static,
    mut child_stderr: impl AsyncRead + Unpin,
) -> anyhow::Result<bool> {
    // Search for the success token
    let mut search_for_token = tokio::task::spawn(
        async move {
            let mut buf = String::new();
            loop {
                let count = child_stdout.read_line(&mut buf).await?;
                debug!(?buf, ?count, "Read line from child proc stdout");

                // eof
                if count == 0 {
                    debug!(?buf, "EOF, did not escalate");
                    return anyhow::Ok(false);
                }

                if buf.contains(SUCCESS_TOKEN) {
                    debug!(?buf, "Found success token");
                    return anyhow::Ok(true);
                }
                buf.clear();
            }
        }
        .instrument(debug_span!("search_for_token")),
    );

    loop {
        tokio::select! {
            found_token = &mut search_for_token => {
                return found_token.unwrap();
            }
            b = (&mut child_stderr).read_u8() => {
                parent_stderr.write_u8(b?).await?;
            }
        }
    }
}
