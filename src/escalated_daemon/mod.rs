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

use tracing::info;
use tracing_unwrap::ResultExt;

use crate::{
    escalated_daemon::ipc::SpawnWriter,
    ipc_common::{read_msg_async, write_msg},
    writer_process::spawn_writer,
};

pub mod ipc;

pub async fn main() {
    loop {
        let (id, msg) = match read_msg_async::<(u64, SpawnWriter)>(tokio::io::stdin()).await {
            Ok(d) => d,
            Err(e) => {
                tracing::info!("Error received on stdin, quitting: {e}");
                return;
            }
        };
        info!(?msg, "Received SpawnWriter request");

        let child = spawn_writer(
            move |m| {
                write_msg(std::io::stdout(), &(id, m)).ok_or_log();
            },
            msg.init_config,
        );
        info!(?child, "Spawned writer thread");
    }
}
