use std::sync::Arc;

use arc_swap::ArcSwap;

/// A type that gets invalidated when ANY of its referents gets dropped.
///
/// Cloning it does not increment any reference counts. However, if the clone is dropped,
/// the original value will be dropped too.
pub struct AnyDrop<T> {
    inner: Arc<ArcSwap<Option<Arc<T>>>>,
}

impl<T> AnyDrop<T> {
    /// Create a new [`AnyDrop`] with the provided value inside.
    ///
    /// To create more references, clone it.
    pub fn new(value: T) -> Self {
        let inner = Arc::new(ArcSwap::new(Arc::new(Some(Arc::new(value)))));
        Self { inner }
    }

    /// Attempt to upgrade this value into a full [Arc].
    /// 
    /// If successful, the [Arc] will live beyond the lifetime of any of the
    /// [AnyDrop]s. In other words, if any of the original [AnyDrop]s are destroyed,
    /// this will also be destroyed.
    pub fn upgrade(&self) -> Option<Arc<T>> {
        let guard = self.inner.load();
        let option: &Option<Arc<T>> = guard.as_ref();
        let option: Option<&Arc<T>> = option.as_ref();
        option.map(|x| x.clone())
    }
}

impl<T> Clone for AnyDrop<T> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

impl<T> Drop for AnyDrop<T> {
    fn drop(&mut self) {
        self.inner.swap(Arc::new(None));
    }
}
