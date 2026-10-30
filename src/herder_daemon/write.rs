use std::{path::PathBuf, time::Duration};

use bytes::Bytes;
use tokio::{
    sync::{mpsc, oneshot, watch},
    time::interval,
};

use crate::{
    codec::compression::CompressionFormat,
    facade::WriteVerifyWorkflow,
    herder_api::write_verify::{WVError, WVEvent},
    util::{
        device,
        io_graph::{self, GraphContext, JunctionTracker, SendJunction, Worker as _},
    },
};

const REFRESH_PERIOD: Duration = Duration::from_millis(10);

#[tracing::instrument(skip_all)]
async fn tracker_coroutine(
    hasher_input_junction: u32,
    js: &JunctionTracker,
    rx_end: &mut oneshot::Receiver<Result<Bytes, WVError>>,
    tx: mpsc::Sender<WVEvent>,
) {
    tracing::debug!(?REFRESH_PERIOD, "starting tracker coroutine");
    let mut interval = interval(REFRESH_PERIOD);

    loop {
        interval.tick().await;

        // log transfers into history
        let hist = js.take_transfers();
        if !hist.is_empty() {
            tracing::trace!(n_txfrs = ?hist.len(), "got multiple transfers to log");
            tx.send_modify(|s| {
                for (j, txfr) in hist {
                    if j == hasher_input_junction {
                        // perform a cumulative sum
                        let last = s.read_bytes_history().last_datapoint().1;
                        s.read_bytes_history
                            .push(txfr.started(), last + txfr.bytes());
                    }
                }
            });
        }

        match rx_end.try_recv() {
            Ok(r) => {
                // done: notify and quit
                tracing::debug!("sending completion notification");
                tx.send_modify(|s| {
                    s.read_bytes_history.push(Instant::now(), s.file_size_bytes);
                    s.result = Some(r.map_err(Arc::from));
                });
                return;
            }
            Err(oneshot::error::TryRecvError::Empty) => {
                // no response: keep going
                tracing::trace!("not complete yet, keep going");
            }
            Err(oneshot::error::TryRecvError::Closed) => {
                panic!("Hash workflow thread dropped its sender!")
            }
        }
    }
}

#[tracing::instrument(skip_all)]
fn run_writer_thread(
    wf: &WriteVerifyWorkflow,
    tx_start: oneshot::Sender<()>,
    ctx: &GraphContext,
    js: &JunctionTracker,
) -> Result<(), WVError> {
    std::thread::scope(move |s| -> Result<(), WVError> {
        // ensure files can be opened
        let file = FileReader::new(&wf.input_file).map_err(|e| WVError::InputFile(e.into()))?;

        let writer = FileWriter::new(&PathBuf::from(&wf.target.name))
            .map_err(|e| WVError::OutputFile(e.into()))?;

        if file.size() < writer.size()
            && (wf.target.target_type == device::Type::Disk
                || wf.target.target_type == device::Type::Partition)
        {
            return Err(WVError::TargetTooSmall {
                in_size: file.size(),
                out_size: writer.size(),
            });
        }

        // construct the other nodes in the graph
        let decompressor = match wf.compression {
            CompressionFormat::Identity => None,
            other => Some(DecompressorWorker::new(other)),
        };

        // calculate pipeline topology and what buffers are needed to connect
        // things
        let (robjs, dobjs, wobjs) = match decompressor {
            Some(d) => {
                let (reader_send, reader_recv) = io_graph::buf(1024);
                let (decomp_send, decomp_recv) = io_graph::buf(1024);

                (
                    (file, reader_send),
                    Some((reader_recv, d, decomp_send)),
                    (decomp_recv, writer),
                )
            }
            None => {
                let (reader_send, reader_recv) = io_graph::buf(1024);

                ((file, reader_send), None, (reader_recv, writer))
            }
        };

        let reader_junction = js.create();

        tracing::info!("notifying with start data");

        // notify the caller of success
        let Ok(()) = tx_start.send(()) else {
            // failed to notify? just return a sentinel
            return Ok(());
        };

        // actually do the thing
        let r = std::thread::Builder::new()
            .name("fileread".into())
            .spawn_scoped(s, move || {
                let (w, tx) = robjs;
                w.run(ctx, SendJunction::new(tx.bind_to_thread(), reader_junction))
            })
            .unwrap();

        let d = dobjs.map(|dobjs| {
            std::thread::Builder::new()
                .name("decompress".into())
                .spawn_scoped(s, move || {
                    let (rx, w, tx) = dobjs;
                    w.run(ctx, (rx.bind_to_thread(), tx.bind_to_thread()))
                })
                .unwrap()
        });

        let w = std::thread::Builder::new()
            .name("write".into())
            .spawn_scoped(s, move || {
                let (rx, w) = wobjs;
                w.run(ctx, rx.bind_to_thread())
            })
            .unwrap();

        tracing::debug!("threads spawned, joining on all");
        r.join()
            .unwrap()
            .map_err(|e| WVError::InputFile(e.into()))?;

        if let Some(d) = d {
            d.join()
                .unwrap()
                .map_err(|e| WVError::Decompress(format!("{e}")))?;
        }

        w.join()
            .unwrap()
            .map_err(|e| WVError::OutputFile(e.into()))?;

        Ok(())
    })
}
