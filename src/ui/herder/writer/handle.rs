use crate::ipc_common::read_msg_async;
use std::pin::Pin;

use tokio::{
    io::{AsyncBufRead, AsyncWrite},
    process::Child,
};

use crate::writer_process::ipc::{ErrorType, InitialInfo, StatusMessage};

/// A very low-level handle for interacting with an initialized writer.
///
/// If this is dropped, the child process inside is killed, if it manages one.
pub struct WriterHandle {
    /// We would like to kill the process on drop, if we are the direct parent of the
    /// process. So, we own a handle to it.
    _child: Option<Child>,
    initial_info: InitialInfo,
    rx: Pin<Box<dyn AsyncBufRead>>,
    _tx: Pin<Box<dyn AsyncWrite>>,
}

impl WriterHandle {
    /// Create a WriterHandle. This should only ever be invoked by the Herder.
    pub(in super::super) fn new(
        child: Option<Child>,
        initial_info: InitialInfo,
        rx: Pin<Box<dyn AsyncBufRead>>,
        tx: Pin<Box<dyn AsyncWrite>>,
    ) -> Self {
        Self {
            _child: child,
            initial_info,
            rx,
            _tx: tx,
        }
    }

    pub async fn next_message(&mut self) -> anyhow::Result<Option<StatusMessage>> {
        // TODO: is this Option even necessary????
        Ok(Some(read_msg_async::<StatusMessage>(&mut self.rx).await?))
    }

    pub fn initial_info(&self) -> &InitialInfo {
        &self.initial_info
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum StartProcessError {
    #[error("Unexpected first status: {0:?}")]
    UnexpectedFirstStatus(StatusMessage),
    #[error("Explicit failure signaled: {0:?}")]
    Failed(Option<ErrorType>),
}

impl core::fmt::Debug for WriterHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Handle")
            .field("child", &self._child)
            .field("initial_info", &self.initial_info)
            .finish()
    }
}
