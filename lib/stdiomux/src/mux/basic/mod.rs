use bytes::Bytes;
use futures::StreamExt as _;
use tokio::{io::AsyncWrite, sync::mpsc};

use crate::{
    frame::{WriteFrameError, simple::SimpleMuxFrame, tokio::FrameWriter},
    mux::ByteStream,
};

pub mod client;
pub mod server;

/// Forward frames from a transmission queue into the given writer.
#[tracing::instrument(skip_all, level = "debug")]
async fn drive_unbounded_txq_tx(
    w: impl AsyncWrite + Unpin,
    mut txq: mpsc::UnboundedReceiver<(u16, Bytes)>,
) -> Result<(), WriteFrameError<SimpleMuxFrame>> {
    let mut w = FrameWriter::new(w);
    while let Some((ch, bs)) = txq.recv().await {
        let f = SimpleMuxFrame {
            channel: ch,
            body: bs,
        };
        tracing::trace!(?f, "transmitting frame");
        w.write_frame(f).await?;
    }
    tracing::debug!("txq driver closing");
    Ok(())
}

#[tracing::instrument(skip_all, level = "debug", fields(?id))]
async fn drive_user_provided_stream(
    mut bs: ByteStream,
    id: u16,
    txq: mpsc::UnboundedSender<(u16, Bytes)>,
) {
    while let Some(f) = bs.next().await {
        // Ignore 0-length frames because that would signal closure
        if f.len() == 0 {
            continue;
        }

        // Attempt to send a frame to the txq
        let Ok(()) = txq.send((id, f)) else {
            // If sending failed, the txq is dropped
            break;
        };
    }

    // No matter what, send one more 0-length frame to signal closure
    tracing::debug!(?id, "user tx driver closing");
    txq.send((id, Bytes::new())).ok();
}
