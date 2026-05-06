//! Helpers for generating a modular I/O graph.

use std::{
    collections::HashMap, io::{Read, Write}, sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    }, time::Instant
};

use super::junction::Junction;

pub struct IoGraphTracker {
    context: Arc<Context>,
    nodes: HashMap<u64, NodeInfo>,
}

pub struct NodeEntry {
    info: NodeInfo,
    outputs: Vec<u64>,
    inputs: Vec<u64>,
}

pub enum NodeKind {
    File,
    Hash,
    Forward,
    Buffer,
}

pub struct IoGraphBuilder {
    next_id: u64,
    nodes: Vec<(u64, NodeInfo)>,
    workers: Vec<(u64, Box<dyn Worker>)>,
}

impl IoGraphBuilder {
    pub fn new() -> Self {
        Self {
            next_id: 0,
            nodes: vec![],
            workers: vec![],
        }
    }

    fn new_id(&mut self) -> u64 {
        let out = self.next_id;
        self.next_id += 1;
        out
    }

    pub fn register_passive_node(&mut self, node: &impl Node) {
        let id = self.new_id();
        self.nodes.push((id, node.info()));
    }

    pub fn register_worker_node(&mut self, worker: impl Worker) {
        let id = self.new_id();
        self.nodes.push((id, worker.info()));
        self.workers.push((id, Box::new(worker)));
    }

    pub fn spawn(self) -> IoGraphTracker {
        let mut map = HashMap::new();
        for (id, w) in self.nodes {
            map.entry(id).or_insert_with(|| NodeEntry { info: w,  inputs: vec![], outputs: vec![] });
        }
    }
}

#[must_use]
pub struct Source<R: Read> {
    pub read: R,
    pub junction: Junction,
}

#[must_use]
pub struct Sink<W: Write> {
    pub write: W,
    pub junction: Junction,
}

#[must_use]
pub trait Node {
    fn info(&self) -> NodeInfo;
}

pub struct NodeInfo {
    pub kind: NodeKind,
    pub input_junctions: Vec<Junction>,
    pub output_junctions: Vec<Junction>,
}

#[must_use]
pub trait Worker: Node + Send + 'static {
    fn run(self, context: Arc<Context>) -> std::io::Result<()>;
}

pub struct Context {
    halt: AtomicBool,
    start_time: Instant,
}

impl Context {
    pub fn new(now: Instant) -> Self {
        Self {
            halt: false.into(),
            start_time: now,
        }
    }

    pub fn halt(&self) -> bool {
        self.halt.load(Ordering::Relaxed)
    }

    pub fn start_time(&self) -> Instant {
        self.start_time
    }
}
