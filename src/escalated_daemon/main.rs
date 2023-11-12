//! This sounds satanic, but it's just a background process running as
//! root, that spawns other processes running as root.
//!
//! The reason we can't just `sudo caligula` writer processes over and
//! over again is because most sudo installations drop your sudo cookie
//! after a while.

use std::io::stdout;

use tokio::{sync::Notify, task::LocalSet};

use crate::{
    ipc_common::{read_msg_async, write_msg},
    writer_process,
};

use super::{SpawnRequest, StatusMessage};

#[tokio::main(flavor = "current_thread")]
pub async fn main() -> anyhow::Result<()> {
    let local = LocalSet::new();

    // Notify to quit in case of fatal error
    let error_notify = Notify::new();

    loop {
        tokio::select! {
            msg = read_msg_async::<SpawnRequest>(tokio::io::stdin()) => {
                let msg = msg.expect("fatal: could not parse error");
                tokio::task::spawn_local(async move {
                    if let Err(e) = spawn(msg).await {
                        write_msg(
                            stdout(),
                            &StatusMessage::FatalError {
                                error: e.to_string(),
                            },
                        );
                        error_notify.notify_waiters();
                    }
                });
            }
            _ = error_notify.notified() => {
                break;
            }
        }
    }

    Ok(())
}

async fn spawn(msg: SpawnRequest) -> anyhow::Result<()> {
    let child_id = msg.child_id;
    let mut h = match writer_process::spawn(&msg.config).await {
        Ok(h) => h,
        Err(e) => {
            write_msg(
                std::io::stdout(),
                &StatusMessage::SpawnFailure {
                    child_id,
                    error: e.to_string(),
                },
            )?;
            return Ok(());
        }
    };

    let code = h.wait().await?;

    write_msg(
        stdout(),
        &StatusMessage::ChildExited {
            child_id,
            code: code.code(),
        },
    );

    Ok(())
}
