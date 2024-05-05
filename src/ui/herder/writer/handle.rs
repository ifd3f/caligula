use crate::ipc_common::read_msg_async;
use std::pin::Pin;

use tokio::{
    io::{AsyncBufRead, AsyncWrite},
    process::Child,
};

use crate::writer_process::ipc::{ErrorType, InitialInfo, StatusMessage};

/// A handle for interacting with an initialized writer.
pub struct WriterHandle {
    _child: Option<Child>,
    initial_info: InitialInfo,
    rx: Pin<Box<dyn AsyncBufRead>>,
    _tx: Pin<Box<dyn AsyncWrite>>,
}

impl WriterHandle {
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
