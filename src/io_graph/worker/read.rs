use std::{io::Read, marker::PhantomData, u64};

use crate::{
    compression::{CompressionFormat, decompress},
    io_graph::{ALLOC_LAYOUT, GraphContext, RecvBytes, SendBytes, Worker, util::RecvBytesReader},
    util::alloc_uninit_bytes_with_layout,
};

pub struct Reader {
    _private: (),
    max_size: Option<u64>,
}

impl Reader {
    pub fn new(max_size: Option<u64>) -> Box<Self> {
        Box::new(Self { max_size, _private: () })
    }
}

impl<R: Read, Tx: SendBytes> Worker<(R, Tx)> for Reader {
    type Error = std::io::Error;
    type Output = ();

    fn run(
        self: Box<Self>,
        context: &GraphContext,
        (mut r, mut tx): (R, Tx),
    ) -> Result<Self::Output, Self::Error> {
        let mut bytes_remaining = self.max_size.unwrap_or(u64::MAX);

        while !context.halt() && bytes_remaining > 0 {
            // SAFETY: these bytes will get filled up immediately. everything else that
            // wasn't filled up will get truncated
            let mut buf = unsafe { alloc_uninit_bytes_with_layout(ALLOC_LAYOUT) };

            buf.truncate(bytes_remaining as usize);

            let count = r.read(&mut buf)?;
            if count == 0 {
                break;
            }

            bytes_remaining -= count as u64;

            buf.truncate(count);
            tx.send(buf.freeze())?;
        }

        tx.close()?;

        Ok(())
    }
}
