#![allow(unused)]
#[cfg(target_os = "macos")]
mod darwin;
mod hidden_input;
mod unix;

use std::io::{self, Read, Write};
use std::{os::fd::AsRawFd, process::Stdio};

use futures::future;
use tokio::io::{
    AsyncBufRead, AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader,
};
use tokio::sync::mpsc;
use tracing::{debug, debug_span, info, Instrument};

use crate::escalation::hidden_input::HiddenInput;

pub use self::unix::Command;
pub use self::unix::EscalationMethod;

/// A token that is used to detect if a process has been escalated.
///
/// If the child process spits this out on stdout that means we escalated successfully.
pub const SUCCESS_TOKEN: &'static str = "znxbnvm,,xbnzcvnmxzv,.,,,";

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("No escalation methods detected! Searched for sudo, doas, su")]
    UnixNotDetected,

    #[cfg(target_os = "macos")]
    #[error("User failed to confirm")]
    MacOSDenial,
}

#[tracing::instrument(skip_all)]
pub async fn run_escalate(
    cmd: &Command<'_>,
    modify: impl FnOnce(&mut tokio::process::Command) -> (),
    em: EscalationMethod,
) -> anyhow::Result<tokio::process::Child> {
    use self::unix::EscalationMethod;

    let wrapped = em.wrap_command(cmd);
    info!(?wrapped, "Constructed wrapped command");

    let mut cmd: tokio::process::Command = wrapped.into();
    modify(&mut cmd);
    // inherit sucks but it's unfortunately the best way to do this for now.
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    info!(?cmd, "Spawning child process");
    let mut proc = cmd.spawn()?;
    let mut stdin = proc.stdin.take().unwrap();
    let mut stdout = BufReader::new(proc.stdout.take().unwrap());
    let mut stderr = proc.stderr.take().unwrap();

    let _hidden = match std::fs::File::open("/dev/tty") {
        Ok(tty) => Some(HiddenInput::new(tty.as_raw_fd())?),
        Err(err) => {
            info!(?err, "Error opening /dev/tty");
            match err.raw_os_error() {
                Some(libc::ENXIO) => {
                    info!("/dev/tty not found, no need to hide input");
                    None
                }
                _ => Err(err)?,
            }
        }
    };

    info!("Starting event loop");
    match event_loop(
        tokio::io::stdin(),
        stdin,
        tokio::io::stderr(),
        stdout,
        stderr,
    )
    .await?
    {
        true => Ok(proc),
        false => anyhow::bail!("Could not escalate"),
    }
}

#[tracing::instrument(skip_all, level = "trace")]
async fn event_loop(
    mut parent_stdin: impl AsyncRead + Unpin,
    mut child_stdin: impl AsyncWrite + Unpin,
    mut parent_stderr: impl AsyncWrite + Unpin,
    mut child_stdout: impl AsyncBufRead + Unpin,
    mut child_stderr: impl AsyncRead + Unpin,
) -> anyhow::Result<bool> {
    // Search for the success token
    let mut search_for_token = async move {
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
    .instrument(debug_span!("search_for_token"));

    #[tracing::instrument(skip_all)]
    async fn fwd(
        mut src: impl AsyncRead + Unpin,
        mut dst: impl AsyncWrite + Unpin,
        bufsize: usize,
    ) -> std::io::Result<()> {
        let mut buf = vec![0u8; bufsize];
        loop {
            let count = src.read(&mut buf).await?;
            if count == 0 {
                info!("Pipe ran out");
                future::pending::<()>().await;
            }
            dst.write(&buf[..count]).await?;
        }
    }

    tokio::select! {
        found_token = search_for_token => {
            return found_token;
        }
        r = fwd(parent_stdin, child_stdin, 1024) => {
            panic!("This future never returns so this should be impossible {r:?}");
        }
        r = fwd(child_stderr, parent_stderr, 1024) => {
            panic!("This future never returns so this should be impossible {r:?}");
        }
    }
}
