use std::io::Write;

use anyhow::Context;
use bincode::Options;
use byteorder::{BigEndian, WriteBytesExt};
use serde::{de::DeserializeOwned, Serialize};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

/// Common bincode options to use for inter-process communication.
#[inline]
pub fn bincode_options() -> impl bincode::Options {
    bincode::DefaultOptions::new()
        .with_fixint_encoding()
        .with_native_endian()
        .with_limit(1024)
}

pub fn write_msg<T: Serialize>(mut w: impl Write, msg: &T) -> anyhow::Result<()> {
    let buf = bincode_options().serialize(msg)?;
    w.write_u32::<BigEndian>(buf.len() as u32)?;
    w.write_all(&buf)?;
    Ok(())
}

pub async fn write_msg_async<T: Serialize>(
    mut w: impl AsyncWrite + Unpin,
    msg: &T,
) -> anyhow::Result<()> {
    let buf = bincode_options().serialize(msg)?;
    w.write_u32(buf.len() as u32).await?;
    w.write_all(&buf).await?;
    Ok(())
}

pub async fn read_msg_async<T: DeserializeOwned>(
    mut r: impl AsyncRead + Unpin,
) -> anyhow::Result<T> {
    let size = r.read_u32().await?;
    let mut buf = vec![0; size as usize];
    r.read_exact(&mut buf).await?;

    let msg: T = bincode_options()
        .deserialize(&buf)
        .context("Failed to parse bincode from stream")?;
    Ok(msg)
}

#[cfg(test)]
mod tests {
    use crate::ipc_common::read_msg_async;

    use super::*;

    #[tokio::test]
    async fn write_read_roundtrip() {
        let messages = &["hsdhjiefhjke", "yveuih3u3rin"];
        let mut buf = Vec::new();

        for msg in messages {
            write_msg(&mut buf, &msg).unwrap();
        }

        let mut reader = &buf[..];
        for msg in messages {
            let out = read_msg_async::<String>(&mut reader).await.unwrap();
            assert_eq!(&out, msg);
        }
    }
}
