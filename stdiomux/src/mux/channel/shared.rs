use std::sync::Arc;

use bytes::Bytes;
use futures::task::AtomicWaker;
use ringbuf::{
    HeapRb,
    traits::{Consumer, Observer, Producer},
};

#[derive(Debug, thiserror::Error)]
#[error("Buffer is full")]
pub struct Full;

#[derive(Debug, thiserror::Error)]
#[error("Buffer is empty")]
pub struct Empty;

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum Mode {
    WakeOnInsert,
    WakeOnRemove,
}

/// Ring buffer combined with a waker.
pub(crate) struct WokeRb<T> {
    pub(crate) buf: Option<HeapRb<T>>,
    pub(crate) is_available: AtomicWaker,
    pub(crate) granted_permits: usize,
    mode: Mode,
}

impl<T> WokeRb<T> {
    pub fn new(size: usize, mode: Mode) -> Self {
        Self {
            buf: match size {
                0 => None,
                other => Some(HeapRb::new(other)),
            },
            is_available: AtomicWaker::new(),
            granted_permits: 0,
            mode,
        }
    }

    /// How many slots are open
    pub fn available_capacity(&self) -> usize {
        self.buf.as_ref().map(|b| b.vacant_len()).unwrap_or(0)
    }

    pub fn try_push(&mut self, x: T) -> Result<(), Full> {
        let b = self.buf.as_mut().ok_or(Full)?;
        b.try_push(x).map_err(|_| Full)?;
        if self.mode == Mode::WakeOnInsert {
            self.is_available.wake();
        }
        Ok(())
    }

    pub fn try_pop(&mut self) -> Result<T, Empty> {
        let b = self.buf.as_mut().ok_or(Empty)?;
        let out = b.try_pop().ok_or(Empty)?;
        if self.mode == Mode::WakeOnRemove {
            self.is_available.wake();
        }
        Ok(out)
    }
}
