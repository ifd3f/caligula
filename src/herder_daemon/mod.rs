//! This module contains the herder daemon process, along with all of the
//! utilities it uses to herd and monitor groups of threads.

// Side note: Interestingly, this interface can theoretically be used to have
// caligula delegate writing to remote hosts over SSH. This may be a very
// strange but funny feature to implement.

use std::convert::Infallible;

use futures::{StreamExt as _, stream::BoxStream};
use tokio::{
    select,
    sync::{mpsc, oneshot},
};
use tokio_stream::wrappers::UnboundedReceiverStream;
use tracing::{debug, info};

use crate::{
    herder_api::{
        HerderResponse, HerderService,
        error::LayerError,
        server::transportize,
        write_verify::{WVAction, WVError, WVEvent},
    },
    util::{
        runtime::{AsyncRuntime, RemoteSpawn as _},
        stdiomux,
        stream::StreamExt,
    },
};

mod writer_process;

pub fn main() {
    AsyncRuntime::start()
        .spawn(|| {
            stdiomux::server::run(
                tokio::io::stdin(),
                tokio::io::stdout(),
                transportize(HerderServer::new()),
            )
        })
        .blocking_recv()
        .expect("Daemon dropped!")
        .expect("Daemon errored!");
}

struct HerderServer {}

impl HerderServer {
    fn new() -> Self {
        Self {}
    }
}

impl HerderService<WVAction> for HerderServer {
    type Error = Infallible;

    #[tracing::instrument(skip_all)]
    async fn start(
        &self,
        action: WVAction,
    ) -> Result<HerderResponse<WVAction, Self::Error>, LayerError<WVError, Self::Error>> {
        info!(?action, "Received WVAction request");

        // needed to communicate start and events
        let (start_tx, start_rx) = oneshot::channel();
        let (ev_tx, ev_rx) = mpsc::unbounded_channel();

        // now spawn this ugly thing
        let mut thread_result_future = Box::pin(writer_process::spawn_writer(
            move |m| {
                start_tx.send(m).ok();
            },
            move |m| {
                ev_tx.send(m).ok();
            },
            action,
        ));
        debug!("Spawned writer thread, waiting for start response");

        // ensure it doesn't error before start
        let start = select! {
            biased;
            r = &mut thread_result_future => {
                return match r {
                    Ok(()) => Err(WVError::UnknownChildProcError("Thread ended without start event".into()))?,
                    Err(e) => Err(LayerError::App(e))
                }
            }
            r = start_rx => {
                r.map_err(|e| WVError::UnknownChildProcError(format!("Failed to receive start result from thread: {e:?}")))?
            }
        };
        info!(?start, "Successfully spawned writer thread");

        // and now to shape it into that stupid type sig
        let events: BoxStream<'static, Result<WVEvent, LayerError<WVError, Infallible>>> =
            UnboundedReceiverStream::new(ev_rx)
                .map(Ok)
                .chain_err_from_future(thread_result_future)
                .map(|r| r.map_err(LayerError::App))
                .boxed();

        Ok(HerderResponse { start, events })
    }
}
