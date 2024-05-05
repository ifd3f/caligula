use crate::{ipc_common::read_msg_async, writer_process::ipc::InitialInfo};
use std::pin::Pin;

use interprocess::local_socket::tokio::{prelude::*, RecvHalf};
use serde::de::DeserializeOwned;
use tokio::{
    io::{AsyncWrite, BufReader},
    process::Child,
};

use crate::writer_process::ipc::StatusMessage;

/// A very low-level handle for interacting with a child process connected to our socket.
///
/// If this is dropped, the child process inside is killed, if it manages one.
pub struct ChildHandle {
    /// We would like to kill the process on drop, if we are the direct parent of the
    /// process. So, we own a handle to it.
    pub(super) child: Option<Child>,
    pub(super) rx: Pin<Box<BufReader<RecvHalf>>>,
    pub(super) _tx: Pin<Box<dyn AsyncWrite>>,
}

impl ChildHandle {
    pub fn new(child: Option<Child>, stream: LocalSocketStream) -> ChildHandle {
        let (rx, tx) = stream.split();
        let rx = Box::pin(BufReader::new(rx));
        let tx = Box::pin(tx);
        Self { child, rx, _tx: tx }
    }

    pub async fn next_message<T: DeserializeOwned>(&mut self) -> anyhow::Result<T> {
        Ok(read_msg_async::<T>(&mut self.rx).await?)
    }
}

impl core::fmt::Debug for ChildHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("Handle").field(&self.child).finish()
    }
}

/// A wrapper around a [ChildHandle] that has sent an [InitialInfo] already.
pub struct WriterHandle {
    pub(super) handle: ChildHandle,
    pub(super) initial_info: InitialInfo,
}

impl WriterHandle {
    pub async fn next_message(&mut self) -> anyhow::Result<Option<StatusMessage>> {
        // TODO: is this Option even necessary????
        Ok(Some(self.handle.next_message().await?))
    }

    pub fn initial_info(&self) -> &InitialInfo {
        &self.initial_info
    }
}
