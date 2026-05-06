use crate::{
    io_graph::{
        builder::{Context, Node, NodeInfo, NodeKind,  Source, Worker},
        junction::TransferStat,
    },
};
use digest::Digest;
use tokio::sync::oneshot;
use std::{
    io::{self, Read},
    marker::PhantomData,
    sync::Arc,
    time::Instant,
};

pub struct HashWorker<R: Read, H: Digest> {
    input: Source<R>,
    result: oneshot::Sender<Bytes>,
    _phantom: PhantomData<H>,
}

impl<R: Read, H: Digest> HashWorker<R, H> {
    pub fn new(input: Source<R>) -> Self {
        Self {
            input,
            _phantom: PhantomData,
        }
    }
}

impl<R: Read, H: Digest> Node for HashWorker<R, H> {
    fn info(&self) -> NodeInfo {
        NodeInfo {
            kind: NodeKind::Forward,
            input_junctions: vec![self.input.junction.clone()],
            output_junctions: vec![],
        }
    }
}

unsafe impl<R: Read + Send, H: Digest> Send for HashWorker<R, H> {}

impl<R: Read + Send + 'static, H: Digest + 'static> Worker for HashWorker<R, H> {
    fn run(mut self, context: Arc<Context>) -> io::Result<()> {
        let mut h = H::new();
        let mut buf = vec![0u8; 4096];
        while !context.halt() {
            let start = Instant::now();
            let count = self.input.read.read(&mut buf)?;
            if count == 0 {
                break;
            }
            self.input.junction.log_transfer(TransferStat::new(
                count as u64,
                start,
                context.start_time(),
            ));

            h.update(&buf[..count]);
        }
        Ok(())
    }
}
