//! This module has logic for the child process that writes to the disk.
//!
//! IT IS NOT TO BE USED DIRECTLY BY THE USER! ITS API HAS NO STABILITY
//! GUARANTEES!

use std::path::Path;

use par::{Session, exchange::Recv, queue::Dequeue};
use tracing::debug;

use crate::{
    codec::compression::CompressionFormat,
    herder_api::write_verify::*,
    util::{
        device::{self},
        io_graph::{
            self, GraphBuilder, GraphStartResult,
            util::{RecvBytesExt, SendBytesExt as _, WorkerExt as _},
            worker,
        },
        phased_channel::{self, UninitializedReceiver},
    },
};

/// Maximum size we may allocate for each buffer.
const MAX_BUF_SIZE: usize = 1 << 20; // 1MiB

/// How many bytes should be written before we perform a checkpoint (aka report
/// progress).
const CHECKPOINT_BYTES: usize = 8 * (1 << 20); // 8MiB

pub enum Event<T, S, E> {
    Done(T),
    Status(S),
    Err(E),
}

pub type WriterProtocol = Recv<WVStart, Dequeue<WriterActive>>;

pub type WriterActive = Dequeue<Event<(), Recv<WVEvent>, Recv<WVError>>>;

/// Spawn a writer/verifier thing. Returns a future to await on its completion.
pub fn spawn_writer(init_config: &WVAction) -> Result<WriterProtocol, WVError> {
    let (tx, rx) = phased_channel::channel();
    let (gsr, start) = run_writer(init_config.compression, &init_config.src, &init_config.dest);

    let receiver: WriterProtocol = WriterProtocol::fork_sync(|tx| {
        std::thread::Builder::new()
            .name("writer".into())
            .spawn(move || {
                debug!("Spawned child thread {:?}", std::thread::current().id());
                let next = tx.send(start);
            })
            .expect("failed to spawn thread");
    });
}

#[tracing::instrument(skip_all)]
fn run_writer(
    compression: CompressionFormat,
    input_file: &Path,
    target_file: &Path,
    target_type: device::Type,
) -> Result<(GraphStartResult<WVError>, WVStart), WVError> {
    let mut builder = GraphBuilder::new();

    // calculate pipeline topology and build buffers
    let (rparms, dparms, wparms) = match compression {
        CompressionFormat::Identity => {
            let (reader_send, hasher_recv) = io_graph::buf(1024);

            (reader_send, None, hasher_recv)
        }
        cf => {
            let (reader_send, decomp_recv) = io_graph::buf(1024);
            let (decomp_send, hasher_recv) = io_graph::buf(1024);

            (
                reader_send,
                Some((decomp_recv, cf, decomp_send)),
                hasher_recv,
            )
        }
    };

    // open reader
    let reader_send = rparms.junctionize_send(builder.new_junction());
    let reader_junction = reader_send.junction().id();
    let (metadata, reader) = worker::file_reader(
        input_file,
        reader_send.junctionize_send(builder.new_junction()),
    )
    .map_err(WVError::from_reader)?;
    let file_size = metadata.len();
    builder.add_worker("reader".into(), reader.map_worker_err(WVError::from_reader));

    // construct decompressor if you must
    if let Some((rx, cf, tx)) = dparms {
        builder.add_worker(
            "decompressor".into(),
            worker::decompressor(cf, rx, tx).map_worker_err(WVError::from),
        );
    }

    // and writer
    let writer_recv = wparms.junctionize_recv(builder.new_junction());
    let writer_junction = writer_recv.junction().id();
    let (_metadata, writer) = worker::file_writer(&target_file, Some(target_type), writer_recv)
        .map_err(WVError::from_writer)?;
    builder.add_worker("writer".into(), writer.map_worker_err(WVError::from_writer));

    let r = builder.start();

    Ok((
        r,
        WVStart {
            input_file_bytes: file_size,
        },
    ))
}
