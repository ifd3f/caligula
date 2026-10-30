use bytes::Bytes;
use digest::Digest;

use crate::util::io_graph::{GraphContext, RecvBytes, Worker};

pub fn hasher<H: Digest>(
    mut rx: impl RecvBytes,
) -> impl Worker<Output = Bytes, Error = std::io::Error> {
    move |ctx: &GraphContext| {
        let mut h = H::new();

        while !ctx.halt() {
            match rx.recv()? {
                Some(b) => h.update(b),
                None => break,
            }
        }

        let h = h.finalize();
        Ok(Bytes::copy_from_slice(&h))
    }
}
