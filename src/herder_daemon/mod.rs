//! This module contains the herder daemon process, along with all of the utilities it uses to
//! herd and monitor groups of threads.

// Side note: Interestingly, this interface can theoretically be used to have caligula delegate
// writing to remote hosts over SSH. This may be a very strange but funny feature to implement.

use tracing::info;
use tracing_unwrap::ResultExt;

use crate::{
    ipc_common::{read_msg_async, write_msg},
    writer_process::spawn_writer,
};

pub mod ipc;

pub async fn main() {
    loop {
        let msg = match read_msg_async::<ipc::StartHerd>(tokio::io::stdin()).await {
            Ok(d) => d,
            Err(e) => {
                tracing::info!("Error received on stdin, quitting: {e}");
                return;
            }
        };
        info!(?msg, "Received StartAction request");

        let child = spawn_writer(
            move |m| {
                write_msg(std::io::stdout(), &(msg.id, m)).ok_or_log();
            },
            msg.action,
        );
        info!(?child, "Spawned writer thread");
    }
}
