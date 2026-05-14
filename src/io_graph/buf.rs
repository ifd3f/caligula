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

#[cfg(test)]
mod tests {
    use bytes::Bytes;

    use super::*;
    use crate::io_graph::{RecvBytes, SendBytes};

    #[test]
    fn send_and_recv_single_message() {
        let (mut tx, mut rx) = buf(4);
        tx.send(Bytes::from("hello")).unwrap();
        assert_eq!(rx.recv().unwrap(), Some(Bytes::from("hello")));
    }

    #[test]
    fn send_and_recv_multiple_messages_in_order() {
        let (mut tx, mut rx) = buf(4);
        let messages = ["foo", "bar", "baz"];
        for m in messages {
            tx.send(Bytes::from(m)).unwrap();
        }
        for m in messages {
            assert_eq!(rx.recv().unwrap(), Some(Bytes::from(m)));
        }
    }

    #[test]
    fn close_signals_eof_to_receiver() {
        let (tx, mut rx) = buf(4);
        tx.close().unwrap();
        assert_eq!(rx.recv().unwrap(), None);
    }

    #[test]
    fn send_then_close_delivers_all_messages_before_eof() {
        let (mut tx, mut rx) = buf(8);
        tx.send(Bytes::from("first")).unwrap();
        tx.send(Bytes::from("second")).unwrap();
        tx.close().unwrap();

        assert_eq!(rx.recv().unwrap(), Some(Bytes::from("first")));
        assert_eq!(rx.recv().unwrap(), Some(Bytes::from("second")));
        assert_eq!(rx.recv().unwrap(), None);
    }

    #[test]
    fn send_empty_bytes_is_silently_dropped() {
        // Empty payload must NOT be forwarded (it is the EOF sentinel).
        let (mut tx, mut rx) = buf(4);
        tx.send(Bytes::new()).unwrap(); // should be a no-op
        tx.send(Bytes::from("real")).unwrap();
        assert_eq!(rx.recv().unwrap(), Some(Bytes::from("real")));
    }

    #[test]
    fn large_payload_round_trips_intact() {
        let payload = Bytes::from(vec![0xABu8; 1_000_000]);
        let (mut tx, mut rx) = buf(1);
        tx.send(payload.clone()).unwrap();
        assert_eq!(rx.recv().unwrap(), Some(payload));
    }

    #[test]
    fn recv_returns_broken_pipe_when_sender_dropped_without_close() {
        let (tx, mut rx) = buf(4);
        drop(tx); // dropped without calling close()
        let err = rx.recv().unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::BrokenPipe);
    }

    #[test]
    fn send_returns_broken_pipe_when_receiver_dropped() {
        let (mut tx, rx) = buf(1);
        drop(rx);
        let err = tx.send(Bytes::from("oops")).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::BrokenPipe);
    }

    #[test]
    fn close_returns_broken_pipe_when_receiver_dropped() {
        let (tx, rx) = buf(1);
        drop(rx);
        let err = tx.close().unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::BrokenPipe);
    }

    #[test]
    fn threaded_producer_consumer_delivers_all_messages() {
        const N: usize = 1_000;
        let (mut tx, mut rx) = buf(16);

        let producer = std::thread::spawn(move || {
            for i in 0..N {
                tx.send(Bytes::from(i.to_string())).unwrap();
            }
            tx.close().unwrap();
        });

        let mut received = Vec::with_capacity(N);
        while let Some(b) = rx.recv().unwrap() {
            received.push(String::from_utf8(b.to_vec()).unwrap());
        }
        producer.join().unwrap();

        assert_eq!(received.len(), N);
        for (i, s) in received.iter().enumerate() {
            assert_eq!(s, &i.to_string());
        }
    }
}
