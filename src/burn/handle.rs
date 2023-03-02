use std::process::Stdio;

use tokio::{
    fs,
    io::AsyncWriteExt,
    process::{Child, ChildStderr, ChildStdout, Command},
};

use super::{
    ipc::{BurnConfig, StatusMessage},
    BURN_ENV,
};

pub struct Handle {
    child: Child,
    child_stdout: ChildStdout,
    pub child_stderr: ChildStderr,
}

impl Handle {
    pub async fn start(args: BurnConfig, escalate: bool) -> anyhow::Result<Self> {
        // Get path to this process
        let proc = fs::read_link("/proc/self/exe").await?;

        let mut cmd = if escalate {
            let mut cmd = Command::new("sudo");
            cmd.arg(proc);
            cmd
        } else {
            Command::new(&proc)
        };

        let mut child = cmd
            .env(BURN_ENV, "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        let mut child_stdin = child.stdin.take().unwrap();
        let child_stdout = child.stdout.take().unwrap();
        let child_stderr = child.stderr.take().unwrap();

        let initial_msg = serde_json::to_string(&args)?;
        child_stdin.write_all(&initial_msg.as_bytes()).await?;
        child_stdin.shutdown().await?;

        Ok(Self {
            child,
            child_stdout,
            child_stderr,
        })
    }

    pub async fn next_message(&mut self) -> anyhow::Result<StatusMessage> {
        todo!()
    }
}
