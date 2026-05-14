
pub struct DecompressorWorker<H: Digest> {
    _phantom: PhantomData<fn() -> H>,
}

impl<H: Digest> DecompressorWorker<H> {
    pub fn new() -> Box<Self> {
        Box::new(Self {
            _phantom: PhantomData,
        })
    }
}

impl<H: Digest, Rx: RecvBytes> Worker<Rx> for DecompressorWorker<H> {
    type Error = std::io::Error;
    type Output = Bytes;

    fn run(
        self: Box<Self>,
        context: &GraphContext,
        args: Rx,
    ) -> Result<Self::Output, Self::Error> {
        let mut rx = args;
        let mut h = H::new();

        while !context.halt() {
            match rx.recv()? {
                Some(b) => h.update(b),
                None => break,
            }
        }

        let h = h.finalize();
        Ok(Bytes::copy_from_slice(&h))
    }
}