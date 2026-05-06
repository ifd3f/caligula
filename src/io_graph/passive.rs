use std::{fs::File, io::BufReader};

use ringbuf::{HeapCons, HeapProd};

use crate::io_graph::builder::{Node, NodeInfo, NodeKind, Sink, Source};

pub struct FileNode {
    pub output: Source<BufReader<File>>,
}

impl Node for FileNode {
    fn info(&self) -> NodeInfo {
        NodeInfo {
            kind: NodeKind::File,
            input_junctions: vec![],
            output_junctions: vec![self.output.junction.clone()],
        }
    }
}

pub struct BufferNode {
    pub input: Sink<HeapProd<u8>>,
    pub output: Source<HeapCons<u8>>,
}

impl Node for BufferNode {
    fn info(&self) -> NodeInfo {
        NodeInfo {
            kind: NodeKind::Buffer,
            input_junctions: vec![self.input.junction.clone()],
            output_junctions: vec![self.input.junction.clone()],
        }
    }
}
