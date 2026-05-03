use bytes::Bytes;
use futures::StreamExt as _;
use tokio::{io::AsyncWrite, sync::mpsc};

use crate::{
    frame::{WriteFrameError, simple::SimpleMuxFrame, tokio::FrameWriter},
    mux::ByteStream,
};

pub mod client;
pub mod server;

async fn drive_tx(
    w: impl AsyncWrite + Unpin,
    mut txq: mpsc::UnboundedReceiver<(u16, Bytes)>,
) -> Result<(), WriteFrameError<SimpleMuxFrame>> {
    let mut w = FrameWriter::new(w);
    while let Some((ch, bs)) = txq.recv().await {
        w.write_frame(SimpleMuxFrame {
            channel: ch,
            body: bs,
        })
        .await?;
    }
    Ok(())
}

async fn drive_user_provided_stream(
    mut bs: ByteStream,
    id: u16,
    txq: mpsc::UnboundedSender<(u16, Bytes)>,
) {
    while let Some(f) = bs.next().await {
        if f.len() == 0 {
            continue;
        }
        let Ok(()) = txq.send((id, f)) else {
            break;
        };
    }
    txq.send((id, Bytes::new())).ok();
}
