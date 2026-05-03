use std::ascii::escape_default;

use tokio::io::{AsyncRead, AsyncReadExt as _, AsyncWrite, AsyncWriteExt as _};

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
