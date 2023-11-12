use process_path::get_executable_path;
use std::pin::Pin;
use std::process::Stdio;
use tokio::io::AsyncRead;
use tokio::io::AsyncWrite;
use tokio::io::BufReader;
use tracing::debug;

use tokio::process::Child;

use crate::escalation::run_escalate;
use crate::escalation::Command;
use crate::run_mode::RunMode;
use crate::run_mode::RUN_MODE_ENV_NAME;

pub struct EscalatedDaemonHandle {
    pub child: Child,
    pub tx: Pin<Box<dyn AsyncWrite>>,
    pub rx: Pin<Box<dyn AsyncRead>>,
}

pub async fn spawn() -> anyhow::Result<EscalatedDaemonHandle> {
    // Get path to this process
    let proc = get_executable_path().unwrap();

    debug!(
        proc = proc.to_string_lossy().to_string(),
        "Read absolute path to this program"
    );

    fn modify_cmd(cmd: &mut tokio::process::Command) {
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .kill_on_drop(true);
    }
    let mut child = run_escalate(
        &(Command {
            envs: vec![(
                RUN_MODE_ENV_NAME.into(),
                RunMode::EscalatedDaemon.as_str().into(),
            )],
            proc: proc.to_string_lossy(),
            args: vec![],
        }),
        |cmd| {
            cmd.stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .kill_on_drop(true);
        },
    )
    .await?;

    let rx = BufReader::new(
        child
            .stdout
            .take()
            .expect("Failed to get stdout of child process"),
    );
    let tx = child
        .stdin
        .take()
        .expect("Failed to get stdin of child process");

    Ok(EscalatedDaemonHandle {
        child,
        rx: Box::pin(rx),
        tx: Box::pin(tx),
    })
}
