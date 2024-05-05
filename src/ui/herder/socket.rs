use std::path::{Path, PathBuf};

use interprocess::local_socket::{tokio::prelude::*, GenericFilePath, ListenerOptions};
use tracing::debug;
use tracing_unwrap::ResultExt;

/// A managed named socket. It gets auto-deleted on drop.
#[derive(Debug)]
pub struct HerderSocket {
    socket_name: PathBuf,
    socket: LocalSocketListener,
}

impl HerderSocket {
    pub async fn new() -> anyhow::Result<Self> {
        let socket_name: PathBuf =
            std::env::temp_dir().join(format!(".caligula-{}.sock", std::process::id()));
        debug!(
            socket_name = format!("{}", socket_name.to_string_lossy()),
            "Creating socket"
        );
        let socket = ListenerOptions::new()
            .name(socket_name.clone().to_fs_name::<GenericFilePath>()?)
            .create_tokio()?;

        Ok(Self {
            socket,
            socket_name,
        })
    }

    pub async fn accept(&mut self) -> anyhow::Result<LocalSocketStream> {
        Ok(self.socket.accept().await?)
    }

    pub fn socket_name(&self) -> &Path {
        &self.socket_name
    }
}

impl Drop for HerderSocket {
    fn drop(&mut self) {
        std::fs::remove_file(&self.socket_name).unwrap_or_log();
    }
}
