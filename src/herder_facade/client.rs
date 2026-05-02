use crate::herder_daemon::ipc::StartHerd;
use crate::herder_facade::DaemonError;
use crate::ipc_common::write_msg_async;
use hyper::client;
use serde::Serialize;
use tokio::io::AsyncWrite;
use tokio::process::Child;
use tracing::info;

/// A very raw, low-level, write-only interface to the herder daemon.
/// Literally doesn't even implement responses.
pub(super) trait HerderClient {
    async fn start_writer<A: Serialize>(&mut self, id: u64, action: A) -> Result<(), DaemonError>;
}

/// A [HerderClient] that doesn't actually spawn the real [HerderClient] until it
/// gets the first request.
pub(super) struct LazyHerderClient<F: HerderClientFactory> {
    factory: F,
    daemon: Option<F::Output>,
}

/// For constructing [HerderClient]s.
///
/// Unfortunately I can't use an AsyncFnOnce raw because then I'll have so many ugly ugly ugly
/// explicit type holes and shit to patch in [LazyHerderClient] so this is the less bad option.
pub(super) trait HerderClientFactory {
    type Output: HerderClient;

    async fn make(&mut self) -> Result<Self::Output, DaemonError>;
}

impl<H, F> HerderClientFactory for F
where
    H: HerderClient,
    F: AsyncFnMut() -> Result<H, DaemonError>,
{
    type Output = H;

    async fn make(&mut self) -> Result<Self::Output, DaemonError> {
        self().await
    }
}

impl<F: HerderClientFactory> LazyHerderClient<F> {
    pub fn new(factory: F) -> Self {
        Self {
            factory,
            daemon: None,
        }
    }

    #[tracing::instrument(skip_all)]
    async fn ensure_daemon(&mut self) -> Result<&mut F::Output, DaemonError> {
        // very ugly but because of Polonius(tm) we have to implement this way

        let has_daemon = self.daemon.is_some();

        if !has_daemon {
            info!("spawning daemon from factory");
            let daemon = self.factory.make().await?;
            self.daemon = Some(daemon);
        }

        Ok(self.daemon.as_mut().expect("This is an impossible state"))
    }
}

impl<F: HerderClientFactory> HerderClient for LazyHerderClient<F> {
    async fn start_writer<A: Serialize>(&mut self, id: u64, action: A) -> Result<(), DaemonError> {
        self.ensure_daemon().await?.start_writer(id, action).await?;
        Ok(())
    }
}

/// A low-level handle to a child process herder daemon.
///
/// If this is dropped, the child process inside is killed, if it manages one.
pub(super) struct RawHerderClient<W: AsyncWrite + Unpin> {
    /// We would like to kill the process on drop, if we are the direct parent of the
    /// process. So, we own a handle to it.
    pub(super) _child: Option<Child>,
    pub(super) tx: W,
}

impl<W: AsyncWrite + Unpin> From<W> for RawHerderClient<W> {
    fn from(tx: W) -> Self {
        Self { tx, _child: None }
    }
}

impl<W: AsyncWrite + Unpin> HerderClient for RawHerderClient<W> {
    async fn start_writer<A: Serialize>(&mut self, id: u64, action: A) -> Result<(), DaemonError> {
        write_msg_async(&mut self.tx, &StartHerd { id, action })
            .await
            .map_err(DaemonError::TransportFailure)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ipc_common::read_msg_async;
    use assert_matches::assert_matches;
    use serde::{Deserialize, Serialize};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::io::duplex;

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    struct MockAction {
        data: String,
    }

    #[tokio::test]
    async fn test_raw_herder_client_start_writer() {
        let (rx, tx) = duplex(1024);
        let mut client = RawHerderClient::from(tx);

        let action = MockAction {
            data: "foobar".into(),
        };
        let id = 42;

        client.start_writer(id, action.clone()).await.unwrap();

        let msg: StartHerd<MockAction> = read_msg_async(rx).await.unwrap();
        assert_eq!(msg.id, id);
        assert_eq!(msg.action, action);
    }

    struct MockHerderClient {
        call_count: Arc<AtomicUsize>,
    }

    impl HerderClient for MockHerderClient {
        async fn start_writer<A: Serialize>(
            &mut self,
            _id: u64,
            _action: A,
        ) -> Result<(), DaemonError> {
            self.call_count.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    #[derive(Debug, Clone)]
    struct LazyHerderClientCallCounters {
        factory_call_count: Arc<AtomicUsize>,
        client_call_count: Arc<AtomicUsize>,
    }

    fn setup_lazy_herder_client_test_harness<C: HerderClient>(
        mut factory_impl: impl FnMut(&LazyHerderClientCallCounters) -> Result<C, DaemonError>,
    ) -> (
        LazyHerderClientCallCounters,
        LazyHerderClient<impl HerderClientFactory>,
    ) {
        let counters = LazyHerderClientCallCounters {
            factory_call_count: Arc::new(AtomicUsize::new(0)),
            client_call_count: Arc::new(AtomicUsize::new(0)),
        };

        let cloned = counters.clone();

        let client = LazyHerderClient::new(move || {
            let cloned = cloned.clone();
            let r = factory_impl(&cloned);
            cloned.factory_call_count.fetch_add(1, Ordering::SeqCst);
            async move { r }
        });

        (counters, client)
    }

    #[tokio::test]
    async fn test_lazy_herder_client() {
        let (counters, mut client) = setup_lazy_herder_client_test_harness(|counters| {
            Ok(MockHerderClient {
                call_count: counters.client_call_count.clone(),
            })
        });

        assert_eq!(counters.factory_call_count.load(Ordering::SeqCst), 0);

        client
            .start_writer(
                1,
                MockAction {
                    data: "test".into(),
                },
            )
            .await
            .unwrap();
        assert_eq!(counters.factory_call_count.load(Ordering::SeqCst), 1);
        assert_eq!(counters.client_call_count.load(Ordering::SeqCst), 1);

        client
            .start_writer(
                2,
                MockAction {
                    data: "test".into(),
                },
            )
            .await
            .unwrap();
        assert_eq!(counters.factory_call_count.load(Ordering::SeqCst), 1);
        assert_eq!(counters.client_call_count.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn test_lazy_herder_client_factory_failure() {
        let (_, mut client) = setup_lazy_herder_client_test_harness(|_| {
            let r: Result<MockHerderClient, DaemonError> = Err(DaemonError::TransportFailure(
                std::io::Error::new(std::io::ErrorKind::Other, "transport unexpectedly closed"),
            ));
            r
        });

        let result = client
            .start_writer(1, MockAction { data: "foo".into() })
            .await;

        assert_matches!(result, Err(DaemonError::TransportFailure(_)));
    }

    #[tokio::test]
    async fn test_lazy_herder_client_retry() {
        let (counters, mut client) = setup_lazy_herder_client_test_harness(|counters| {
            if counters.factory_call_count.load(Ordering::SeqCst) == 0 {
                let result: Result<MockHerderClient, DaemonError> =
                    Err(DaemonError::TransportFailure(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        "first call fails",
                    )));
                result
            } else {
                Ok(MockHerderClient {
                    call_count: counters.client_call_count.clone(),
                })
            }
        });

        // First call should call factory and fail
        let r1 = client
            .start_writer(1, MockAction { data: "foo".into() })
            .await;
        assert!(r1.is_err());
        assert_eq!(counters.factory_call_count.load(Ordering::SeqCst), 1);
        assert_eq!(counters.client_call_count.load(Ordering::SeqCst), 0);

        // Second call should call factory and succeed
        let r2 = client
            .start_writer(2, MockAction { data: "foo".into() })
            .await;
        assert!(r2.is_ok());
        assert_eq!(counters.factory_call_count.load(Ordering::SeqCst), 2);
        assert_eq!(counters.client_call_count.load(Ordering::SeqCst), 1);

        // Third call should call daemon and succeed
        let r2 = client
            .start_writer(2, MockAction { data: "foo".into() })
            .await;
        assert!(r2.is_ok());
        assert_eq!(counters.factory_call_count.load(Ordering::SeqCst), 2);
        assert_eq!(counters.client_call_count.load(Ordering::SeqCst), 2);
    }
}
