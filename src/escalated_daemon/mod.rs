//! This sounds satanic, but it's just a background process running as
//! root, that spawns other processes running as root.
//!
//! The reason we can't just `sudo caligula` writer processes over and
//! over again is because most sudo installations drop your sudo cookie
//! after a while.

use std::{collections::HashMap, io::stdout, process::exit, rc::Rc, sync::Arc};

use serde::{Deserialize, Serialize};
use tokio::{
    sync::{mpsc, Notify},
    task::{JoinHandle, LocalSet},
};

use crate::{
    ipc_common::{read_msg_async, write_msg},
    writer_process::{self, ipc::WriterProcessConfig},
};

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq)]
pub struct SpawnRequest {
    child_id: u64,
    config: WriterProcessConfig,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq)]
pub enum StatusMessage {
    SpawnFailure { child_id: u64, error: String },
    ChildExited { child_id: u64, code: Option<i32> },
    FatalError { error: String },
}

#[tokio::main(flavor = "current_thread")]
pub async fn main() -> anyhow::Result<()> {
    let local = LocalSet::new();
    let error_notify = Notify::new();

    loop {
        tokio::select! {
            msg = read_msg_async::<SpawnRequest>(tokio::io::stdin()) => {
                let msg = msg.expect("fatal: could not parse error");
            }
        }
        let msg = read_msg_async::<SpawnRequest>(tokio::io::stdin()).await?;

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
}

async fn spawn(msg: SpawnRequest) -> anyhow::Result<()> {
    let child_id = msg.child_id;
    let mut h = match writer_process::Handle::start(&msg.config, false).await {
        Ok(h) => h,
        Err(e) => {
            write_msg(
                std::io::stdout(),
                &StatusMessage::SpawnFailure {
                    child_id: child_id,
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
