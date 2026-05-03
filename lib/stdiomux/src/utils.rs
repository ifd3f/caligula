use std::sync::Arc;

use tokio::{
    io::{AsyncRead, AsyncReadExt as _, AsyncWrite, AsyncWriteExt as _},
    sync::watch,
};

#[derive(Debug, thiserror::Error)]
pub enum HandshakeError {
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error(
        "Received incompatible handshake! The other end is not speaking the same protocol.\n
        Expected: {expected}\n
          Actual: {actual}"
    )]
    Incompatible { expected: String, actual: String },
}

/// Helper function for exchanging a handshake.
pub(crate) async fn exchange_handshake<R, W>(
    mut r: R,
    mut w: W,
    our_handshake: &[u8],
    expected_handshake: &[u8],
) -> Result<(), HandshakeError>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    // write and flush our end
    w.write_all(our_handshake).await?;
    w.flush().await?;

    // read their handshake
    let mut buf = vec![0u8; expected_handshake.len()];
    r.read_exact(&mut buf).await?;

    // validate
    if buf != expected_handshake {
        Err(HandshakeError::Incompatible {
            expected: expected_handshake.escape_ascii().to_string(),
            actual: buf.escape_ascii().to_string(),
        })?
    }

    Ok(())
}

/// Standard length of handshakes for protocols in this library.
pub const HANDSHAKE_LEN: usize = 64;

/// Generates a [`HANDSHAKE_LEN`]-length string with the crate's version in it.
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
        ::byte_strings::const_concat_bytes!(
            START,
            &[0u8; crate::utils::HANDSHAKE_LEN - START.len()]
        )
    }};
}
pub(crate) use make_hello_with_crate_version;

/// Helper for broadcasting an error to multiple locations.
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
    /// Create a new [`AnnounceError`] initialized to not have an error.
    pub fn new() -> Self {
        let (error_tx, error) = watch::channel(Ok(()));
        Self { error, error_tx }
    }

    /// Announce the error.
    ///
    /// Won't overwrite existing errors.
    pub fn announce(&self, e: E) -> Arc<E> {
        let e = Arc::new(e);
        self.announce_arc(e.clone());
        e
    }

    /// If the provided result is an error, announces it and re-returns the error as an [Arc].
    /// Otherwise, does nothing.
    ///
    /// Won't overwrite existing errors.
    pub fn announce_result<T>(&self, r: Result<T, E>) -> Result<T, Arc<E>> {
        match r {
            Ok(ok) => Ok(ok),
            Err(e) => Err(self.announce(e)),
        }
    }

    /// Announce an error that's already an [Arc].
    ///
    /// Won't overwrite existing errors.
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

    /// Wait for an error to occur.
    #[expect(unused)]
    pub async fn wait(&self) -> Arc<E> {
        let mut rx = self.error.clone();
        rx.wait_for(|x| x.is_err())
            .await
            .unwrap()
            .clone()
            .unwrap_err()
    }

    /// Assert that no error has occurred. Returns an error if it has.
    pub fn assert_ok(&self) -> Result<(), Arc<E>> {
        self.error_tx.borrow().clone()
    }
}
