use std::collections::HashMap;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;

use interprocess::local_socket::tokio::LocalSocketListener;
use tokio::io::AsyncRead;
use tokio::io::AsyncWrite;

use crate::escalated_daemon;
use crate::escalated_daemon::EscalatedDaemonHandle;
use crate::escalated_daemon::SpawnRequest;
use crate::ipc_common::write_msg_async;
use crate::writer_process;
use crate::writer_process::ipc::WriterProcessConfig;

pub struct MultiWriterManager {
    socket: LocalSocketListener,
    state: tokio::sync::Mutex<State>,
    next_id: AtomicU64,
}

struct State {
    escalation_handle: Option<EscalatedDaemonHandle>,
    writers: HashMap<u64, Writer>,
}

enum Writer {
    Spawning,
    Spawned(SpawnedWriter),
}

pub struct SpawnedWriter {
    pub id: u64,
    pub rx: Box<dyn AsyncRead + Unpin>,
    pub tx: Box<dyn AsyncWrite + Unpin>,
}

impl MultiWriterManager {
    pub fn new() -> anyhow::Result<Self> {
        let sockname = format!(".caligula-{}", std::process::id());
        let socket = LocalSocketListener::bind(sockname)?;

        Ok(Self {
            socket,
            next_id: 0.into(),
            state: State {
                escalation_handle: None,
                writers: HashMap::new(),
            }
            .into(),
        })
    }

    pub async fn spawn(&self, config: WriterProcessConfig) -> Result<u64, SpawnError> {
        let child_id = self.make_child_id();
        writer_process::spawn(config);

        let mut state = self.state.lock().await;
        state.writers.insert(child_id, Writer::Spawning);
        Ok(child_id)
    }

    pub async fn spawn_escalated(&self, config: WriterProcessConfig) -> Result<u64, SpawnError> {
        let mut state = self.state.lock().await;
        let h = match &mut state.escalation_handle {
            Some(h) => h,
            _ => return Err(SpawnError::NotEscalated),
        };

        let child_id = self.make_child_id();
        write_msg_async(&mut h.tx, &SpawnRequest { child_id, config }).await;
        state.writers.insert(child_id, Writer::Spawning);

        Ok(child_id)
    }

    fn make_child_id(&self) -> u64 {
        self.next_id.fetch_add(1, Ordering::Relaxed)
    }

    pub async fn start_escalated_daemon(&mut self) {
        let mut state = self.state.lock().await;
        state.escalation_handle = Some(escalated_daemon::spawn().await.unwrap());
    }

    pub async fn accept_child(&self) {
        let x = self.socket.accept().await.unwrap();
        let (rx, tx) = x.into_split();
    }
}

pub enum SpawnError {
    NotEscalated,
}
