//! This module contains the herder daemon process, along with all of the
//! utilities it uses to herd and monitor groups of threads.

// Side note: Interestingly, this interface can theoretically be used to have
// caligula delegate writing to remote hosts over SSH. This may be a very
// strange but funny feature to implement.

use std::convert::Infallible;

use futures::{StreamExt as _, stream::BoxStream};
use tracing::{debug, info};

use crate::{
    herder_api::{
        HerderResponse, HerderService,
        error::LayerError,
        server::transportize,
        write_verify::{WVAction, WVError, WVEvent},
    },
    util::{
        phased_channel::ChannelDisconnected,
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

        // now spawn this ugly thing
        let (rx, _) = writer_process::spawn_writer(action);
        debug!("Spawned writer thread, waiting for start response");

        let (start, rx) = match rx.await {
            Ok(Ok(x)) => x,
            Ok(Err(e)) => {
                return Err(LayerError::App(e));
            }
            Err(ChannelDisconnected) => {
                return Err(LayerError::App(WVError::UnknownChildProcError(
                    "failed to spawn".into(),
                )));
            }
        };

        info!(?start, "Successfully spawned writer thread");

        // and now to shape it into that stupid type sig
        let events: BoxStream<'static, Result<WVEvent, LayerError<WVError, Infallible>>> =
            rx.map(|x| x.map_err(LayerError::App)).boxed();

        Ok(HerderResponse { start, events })
    }
}
