use std::sync::mpsc;

use bytes::Bytes;

use crate::io_graph::{RecvBytes, SendBytes};

/// Create a new paired [`BufSender`] and [`BufReceiver`].
/// 
/// This is backed by a channel, with initial capacity set to `channel_size`.
pub fn buf(channel_size: usize) -> (BufSender, BufReceiver) {
    let (tx, rx) = mpsc::sync_channel(channel_size);
    (BufSender { tx }, BufReceiver { rx })
}

#[must_use]
pub struct BufReceiver {
    rx: mpsc::Receiver<Bytes>,
}

impl RecvBytes for BufReceiver {
    fn recv(&mut self) -> std::io::Result<Option<Bytes>> {
        // see if we need to read from the buffer
        let msg = self.rx.recv().map_err(|_| {
            std::io::Error::new(std::io::ErrorKind::BrokenPipe, "mpsc sender was dropped")
        })?;

        // 0 len means close
        if msg.is_empty() {
            return Ok(None);
        }

        Ok(Some(msg))
    }
}

#[must_use]
pub struct BufSender {
    /// tx handle to receiver end. Send 0 bytes to signal EOF.
    tx: mpsc::SyncSender<Bytes>,
}

impl SendBytes for BufSender {
    fn send(&mut self, bytes: Bytes) -> std::io::Result<()> {
        if bytes.is_empty() {
            // don't send 0 bytes because that signals close
            return Ok(());
        }

        self._send(bytes)?;
        Ok(())
    }

    fn close(mut self) -> std::io::Result<()> {
        self._send(Bytes::new())?;
        Ok(())
    }
}

impl BufSender {
    fn _send(&mut self, bytes: Bytes) -> std::io::Result<()> {
        self.tx.send(bytes).map_err(|_| {
            std::io::Error::new(std::io::ErrorKind::BrokenPipe, "mpsc receiver was dropped")
        })?;
        Ok(())
    }
}
