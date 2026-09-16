use std::{
    pin::Pin,
    task::{Context, Poll},
};

use futures::{FutureExt, Stream, StreamExt};
use tokio::sync::{mpsc, oneshot};
use tokio_stream::wrappers::UnboundedReceiverStream;

/// A two-phase message channel. There is an initial phase, and an
/// event-emitting phase. For more info, see [`channel()`].
///
/// ## Examples
///
/// Successful initialization:
///
/// ```
/// ```
pub fn channel<T, V, E>() -> (UninitializedSender<T, V, E>, UninitializedReceiver<T, V, E>) {
    let (itx, irx) = oneshot::channel::<Result<T, E>>();
    let (stx, srx) = mpsc::unbounded_channel::<V>();

    (
        UninitializedSender {
            initial: itx,
            stream: stx,
        },
        UninitializedReceiver {
            initial: irx,
            stream: Some(srx),
        },
    )
}

#[derive(Debug, thiserror::Error)]
#[error("The other end disconnected")]
pub struct ChannelDisconnected;

/// A two-phase sender. There is an initial phase, and an event-emitting phase.
/// For more info, see [`channel()`].
pub struct UninitializedReceiver<T, V, E> {
    initial: oneshot::Receiver<Result<T, E>>,
    stream: Option<mpsc::UnboundedReceiver<V>>,
}

impl<T, V, E> Future for UninitializedReceiver<T, V, E> {
    type Output = Result<Result<(T, InitializedReceiver<V>), E>, ChannelDisconnected>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let Poll::Ready(r) = self.initial.poll_unpin(cx) else {
            return Poll::Pending;
        };

        let stream = self.stream.take().expect("stream taken already!");
        let ir = InitializedReceiver {
            stream: UnboundedReceiverStream::new(stream),
        };
        Poll::Ready(
            r.map_err(|_| ChannelDisconnected)
                .map(|r| r.map(|i| (i, ir))),
        )
    }
}

/// A two-phase receiver. There is an initial phase, and an event-emitting
/// phase. For more info, see [`channel()`].
pub struct InitializedReceiver<V> {
    stream: UnboundedReceiverStream<V>,
}

impl<V> Stream for InitializedReceiver<V> {
    type Item = V;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.stream.poll_next_unpin(cx)
    }
}

pub struct UninitializedSender<T, V, E> {
    initial: oneshot::Sender<Result<T, E>>,
    stream: mpsc::UnboundedSender<V>,
}

impl<T, V, E> UninitializedSender<T, V, E> {
    pub fn send(self, x: T) -> Result<InitializedSender<V>, ChannelDisconnected> {
        self.initial.send(Ok(x)).map_err(|_| ChannelDisconnected)?;
        Ok(InitializedSender {
            stream: self.stream,
        })
    }

    pub fn error(self, e: E) -> Result<(), ChannelDisconnected> {
        self.initial.send(Err(e)).map_err(|_| ChannelDisconnected)?;
        Ok(())
    }
}

pub struct InitializedSender<V> {
    stream: mpsc::UnboundedSender<V>,
}

impl<V> InitializedSender<V> {
    pub fn send(&self, x: V) -> Result<(), ChannelDisconnected> {
        self.stream.send(x).map_err(|_| ChannelDisconnected)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::convert::Infallible;

    use tokio::join;

    use super::*;

    #[tokio::test]
    async fn run() {
        let (tx, rx) = channel::<_, _, Infallible>();

        join!(
            async move {
                let tx = tx.send("foo").unwrap();

                tx.send(1).unwrap();
                tx.send(2).unwrap();
            },
            async move {
                let (first, mut rx) = rx.await.unwrap().unwrap();

                assert_eq!(first, "foo");
                assert_eq!(rx.next().await, Some(1));
                assert_eq!(rx.next().await, Some(2));
                assert_eq!(rx.next().await, None);
            },
        );
    }

    #[tokio::test]
    async fn error_path() {
        let (tx, rx) = channel::<Infallible, Infallible, _>();

        join!(
            async move {
                tx.error("broke!").unwrap();
            },
            async move {
                let result = rx.await.unwrap();

                assert!(result.is_err());
            },
        );
    }
}
