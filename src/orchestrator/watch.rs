use std::{fmt::Debug, ops::Deref};
use tokio::sync::watch;

/// A handle for you to watch state changes.
///
/// This is used because state change events may arrive from the child process at a
/// much faster rate than a UI should reasonably draw them. On UI updates, you can
/// query this object for new
///
/// Technically speaking, this is just a thin wrapper around [`watch::Receiver<S>`].
/// We may change the underlying implementation of this later, so I'm wrapping it
/// like so in order to prevent us from needing to do more refactors later.
#[derive(Clone)]
pub struct Watch<S> {
    pub(super) rx: watch::Receiver<S>,
}

impl<S: Debug> Debug for Watch<S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("WatchState").field(&*self.borrow()).finish()
    }
}

/// A borrowed reference from a [`Watch`].
pub struct Ref<'a, S> {
    r: watch::Ref<'a, S>,
}

#[derive(Debug, thiserror::Error)]
#[error("Worker halted before state reached the expected condition!")]
pub struct ClosedEarly;

impl<S> Watch<S> {
    /// Returns a reference to the most recent state.
    ///
    /// WARNING: Outstanding borrows hold a read lock on the inner value! If you hold this,
    /// the state reducer may not be able to update the state!
    pub fn borrow(&self) -> Ref<'_, S> {
        Ref {
            r: self.rx.borrow(),
        }
    }

    /// Wait until the state matches the given predicate.
    ///
    /// Fails if state never matches the predicate.
    #[expect(dead_code)]
    pub async fn wait_until(
        &mut self,
        pred: impl FnMut(&S) -> bool,
    ) -> Result<Ref<'_, S>, ClosedEarly> {
        let r = self.rx.wait_for(pred).await.map_err(|_| ClosedEarly)?;
        Ok(Ref { r })
    }
}

impl<'a, S> Deref for Ref<'a, S> {
    type Target = S;

    fn deref(&self) -> &Self::Target {
        self.r.deref()
    }
}
