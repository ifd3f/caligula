use std::sync::Arc;

use tokio::sync::watch;

/// Generates a string with the crate's version in it, suitable for validating handshakes.
macro_rules! make_hello_with_crate_version {
    ($controller_name:expr) => {{
        const START: &'static [u8] = concat!(
            env!("CARGO_CRATE_NAME"),
            " ",
            env!("CARGO_PKG_VERSION"),
            " ",
            $controller_name
        )
        .as_bytes();
        ::byte_strings::const_concat_bytes!(START, &[0u8; libc::PIPE_BUF - START.len()])
    }};
}
pub(crate) use make_hello_with_crate_version;

pub(crate) struct AnnounceError<E> {
    error: watch::Receiver<Result<(), Arc<E>>>,
    error_tx: watch::Sender<Result<(), Arc<E>>>,
}

impl<E> Clone for AnnounceError<E> {
    fn clone(&self) -> Self {
        Self {
            error: self.error.clone(),
            error_tx: self.error_tx.clone(),
        }
    }
}

impl<E> AnnounceError<E> {
    pub fn new() -> Self {
        let (error_tx, error) = watch::channel(Ok(()));
        Self { error, error_tx }
    }

    pub fn announce(&self, e: E) -> Arc<E> {
        let e = Arc::new(e);
        self.announce_arc(e.clone());
        e
    }

    pub fn announce_result<T>(&self, r: Result<T, E>) -> Result<T, Arc<E>> {
        match r {
            Ok(ok) => Ok(ok),
            Err(e) => Err(self.announce(e)),
        }
    }

    pub fn announce_arc(&self, e: Arc<E>) {
        self.error_tx.send_if_modified(move |existing| {
            if existing.is_ok() {
                *existing = Err(e);
                true
            } else {
                false
            }
        });
    }

    pub async fn wait(&self) -> Arc<E> {
        let mut rx = self.error.clone();
        rx.wait_for(|x| x.is_err())
            .await
            .unwrap()
            .clone()
            .unwrap_err()
    }

    pub fn assert_ok(&self) -> Result<(), Arc<E>> {
        self.error_tx.borrow().clone()
    }
}
