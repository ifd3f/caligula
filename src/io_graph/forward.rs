use std::{
    io::{self, Read, Write},
    sync::Arc,
    time::Instant,
};

use crate::io_graph::{
    builder::{Context, Node, NodeInfo, NodeKind, Sink, Source, Worker},
    junction::TransferStat,
};

pub struct ForwardWorker<R: Read, W: Write> {
    input: Source<R>,
    output: Sink<W>,
}

impl<R: Read, W: Write> ForwardWorker<R, W> {
    pub fn new(input: Source<R>, output: Sink<W>) -> Self {
        Self { input, output }
    }
}

impl<R: Read, W: Write> Node for ForwardWorker<R, W> {
    fn info(&self) -> NodeInfo {
        NodeInfo {
            kind: NodeKind::Forward,
            input_junctions: vec![self.input.junction.clone()],
            output_junctions: vec![self.output.junction.clone()],
        }
    }
}

impl<R: Read + Send + 'static, W: Write + Send + 'static> Worker for ForwardWorker<R, W> {
    fn run(mut self, context: Arc<Context>) -> io::Result<()> {
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

            let start = Instant::now();
            let count = self.output.write.write(&buf[..count])?;
            self.output.junction.log_transfer(TransferStat::new(
                count as u64,
                start,
                context.start_time(),
            ));
        }
        Ok(())
    }
}
