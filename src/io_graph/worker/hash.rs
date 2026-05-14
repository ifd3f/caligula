use std::marker::PhantomData;

use bytes::Bytes;
use digest::Digest;

use crate::io_graph::{GraphContext, RecvBytes, Worker};

pub struct HashWorker<H: Digest, R: RecvBytes> {
    input: R,
    _phantom: PhantomData<fn() -> H>,
}

impl<H: Digest, R: RecvBytes + Send> HashWorker<H, R> {
    pub fn new(input: R) -> Box<Self> {
        Box::new(Self {
            input,
            _phantom: PhantomData,
        })
    }
}

impl<H: Digest, R: RecvBytes + Send> Worker for HashWorker<H, R> {
    type Error = std::io::Error;
    type Output = Bytes;

    fn run(mut self: Box<Self>, context: &GraphContext) -> Result<Self::Output, Self::Error> {
        let mut h = H::new();
        while !context.halt() {
            match self.input.recv()? {
                Some(b) => h.update(b),
                None => break,
            }
        }
        let h = h.finalize();
        Ok(Bytes::copy_from_slice(&h))
    }
}
