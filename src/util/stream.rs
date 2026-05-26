use std::pin::pin;

use futures::{Stream, StreamExt as _, stream::BoxStream};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

/// Given a [`Stream`] that might not be thread-safe, creates a handle for it
/// that is thread-safe and a driver that must be run on the current thread to
/// forward data over.
pub fn forward_stream_remotely<S, T>(
    stream: S,
    buffer: usize,
) -> (BoxStream<'static, T>, impl Future<Output = ()>)
where
    T: Send + 'static,
    S: Stream<Item = T> + 'static,
{
    let (stream_tx, stream_rx) = mpsc::channel(buffer);

    let out = ReceiverStream::new(stream_rx).boxed();

    let fut = async move {
        let mut stream = pin!(stream);
        while let Some(val) = stream.next().await {
            let Ok(()) = stream_tx.send(val).await else {
                // other end dropped
                break;
            };
        }
    };

    (out, fut)
}
