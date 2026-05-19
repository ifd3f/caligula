//! This module contains the herder daemon process, along with all of the
//! utilities it uses to herd and monitor groups of threads.

// Side note: Interestingly, this interface can theoretically be used to have
// caligula delegate writing to remote hosts over SSH. This may be a very
// strange but funny feature to implement.

use std::convert::Infallible;

use futures::TryStreamExt;
use tokio::sync::{mpsc, oneshot};
use tokio_stream::wrappers::UnboundedReceiverStream;
use tracing::{debug, info};

use crate::{
    herder_api::{
        HerderResponse, HerderService,
        error::LayerError,
        server::transportize,
        write_verify::{WVAction, WVError},
    },
    util::{
        runtime::{AsyncRuntime, RemoteSpawn as _},
        stdiomux,
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

        let (start_tx, start_rx) = oneshot::channel();
        let (ev_tx, ev_rx) = mpsc::unbounded_channel();

        let child = writer_process::spawn_writer(
            move |m| {
                start_tx.send(m).ok();
            },
            move |m| {
                ev_tx.send(m).ok();
            },
            action,
        );
        debug!(?child, "Spawned writer thread, waiting for start response");

        let start = start_rx.await.map_err(|_| WVError::UnexpectedTermination)?;
        info!(?child, ?start, "Successfully spawned writer thread");

        Ok(HerderResponse {
            start,
            events: Box::pin(UnboundedReceiverStream::new(ev_rx).map_err(LayerError::App)),
        })
    }
}
