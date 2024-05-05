//! This sounds satanic, but it's really just a background process running as root,
//! that spawns other processes running as root. The point is so that we can spawn
//! multiple writers running as root while only executing sudo once.
//!
//! The reason we can't just `sudo caligula` writer processes over and over again
//! is because most desktop sudo installations (rightfully) drop your sudo cookie
//! after a while and it would suck to have to repeatedly enter in your password
//! when you only really need to do it once.
//!
//! Given that this is running in root, we would like to restrict its interface as
//! much as possible. In the future, it may even be worthwhile to harden the IPC
//! even further.
//!
//! IT IS NOT TO BE USED DIRECTLY BY THE USER! ITS API HAS NO STABILITY GUARANTEES!

use anyhow::Context;
use interprocess::local_socket::{tokio::prelude::*, GenericFilePath};
use tokio::io::{AsyncBufRead, BufReader};
use tracing::{error, info, info_span, Instrument};
use tracing_unwrap::ResultExt;
use valuable::Valuable;

use crate::{
    childproc_common::child_init,
    escalated_daemon::ipc::{EscalatedDaemonInitConfig, SpawnWriter},
    ipc_common::read_msg_async,
    run_mode::make_writer_spawn_command,
};

pub mod ipc;

#[tokio::main(flavor = "current_thread")]
pub async fn main() {
    let (sock, _) = child_init::<EscalatedDaemonInitConfig>();

    info!("Opening socket {sock}");
    let stream = LocalSocketStream::connect(
        sock.as_str()
            .to_fs_name::<GenericFilePath>()
            .unwrap_or_log(),
    )
    .await
    .unwrap_or_log();

    event_loop(&sock, BufReader::new(stream))
        .await
        .unwrap_or_log();
}

#[tracing::instrument(skip_all)]
async fn event_loop(socket: &str, mut stream: impl AsyncBufRead + Unpin) -> anyhow::Result<()> {
    loop {
        let msg = read_msg_async::<SpawnWriter>(&mut stream).await?;
        info!(msg = msg.as_value(), "Received SpawnWriter request");

        let command =
            make_writer_spawn_command(socket.into(), msg.log_file.into(), &msg.init_config);
        let mut cmd = tokio::process::Command::from(command);
        cmd.kill_on_drop(true);
        let mut child = cmd.spawn().context("Failed to spawn writer process")?;
        info!(?child, "Spawned writer process");

        // Wait on child processes to reap them when they're done.
        let pid = child.id();
        tokio::spawn(
            async move {
                match child.wait().await {
                    Ok(r) => info!("Child exited with exit code {r}"),
                    Err(e) => error!("Failed to wait on child: {e}"),
                }
            }
            .instrument(info_span!("childwait", child_pid = pid)),
        );
    }
}
