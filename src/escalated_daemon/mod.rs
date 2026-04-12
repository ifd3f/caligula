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

use interprocess::local_socket::{GenericFilePath, tokio::prelude::*};
use tokio::io::{AsyncBufRead, BufReader};
use tracing::info;
use tracing_unwrap::ResultExt;

use crate::{
    childproc_common::child_init,
    escalated_daemon::ipc::{EscalatedDaemonInitConfig, SpawnWriter},
    ipc_common::read_msg_async,
    writer_process::spawn_writer,
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
        info!(?msg, "Received SpawnWriter request");

        let child = spawn_writer(socket.into(), msg.init_config);
        info!(?child, "Spawned writer thread");
    }
}
