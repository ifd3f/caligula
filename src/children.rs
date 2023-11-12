use std::{collections::HashMap, pin::Pin};

use interprocess::local_socket::tokio::LocalSocketListener;
use tokio::io::AsyncRead;
use tokio::io::AsyncWrite;

use crate::escalated_daemon;
use crate::escalated_daemon::EscalatedDaemonHandle;
use crate::escalated_daemon::SpawnRequest;
use crate::ipc_common::write_msg_async;
use crate::writer_process::ipc::WriterProcessConfig;

pub struct Children {
    socket: LocalSocketListener,
    state: tokio::sync::Mutex<State>,
}

struct State {
    handle: Option<EscalatedDaemonHandle>,
    writers: HashMap<u64, Writer>,
    next_id: u64,
}

enum Writer {
    Spawning, Spawned(SpawnedWriter)
}

pub struct SpawnedWriter {
    pub id: u64,
    pub rx: Box<dyn AsyncRead + Unpin>,
    pub tx: Box<dyn AsyncWrite + Unpin>,
}

impl Children {
    pub async fn spawn(&self, escalate: bool, config: WriterProcessConfig) -> u64 {
        let child_id = {
            let mut state = self.state.lock().await;
            let out = state.next_id;
            state.next_id += 1;
            out
        };

        if escalate {
            let mut state = self.state.lock().await;
            if let Some(h) = &mut state.handle {
                write_msg_async(&mut h.tx, &SpawnRequest { child_id, config }).await;
            } else {

            }
        } else {
        }

        child_id
    }

    pub async fn start_escalated_daemon(&mut self) {
        let x = escalated_daemon::spawn().await.unwrap();
    }

    pub async fn accept_child(&self) {
        let x = self.socket.accept().await.unwrap();
        let (rx, tx) = x.into_split();

        SpawnedWriter {
            rx: Box::new(rx),
            tx: Box::new(tx),
        }
    }
}
