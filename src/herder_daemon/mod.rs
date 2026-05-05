//! This module contains the herder daemon process, along with all of the utilities it uses to
//! herd and monitor groups of threads.

// Side note: Interestingly, this interface can theoretically be used to have caligula delegate
// writing to remote hosts over SSH. This may be a very strange but funny feature to implement.

use tokio::io::{BufReader, BufWriter};
use tracing::info;
use tracing_unwrap::ResultExt;

use crate::{
    herder_api::{
        HerdAction, HerdEvent, HerderService, StartHerd, TopLevelHerdEvent,
        write_verify::WriteVerifyAction,
    },
    ipc_common::{read_msg_async, write_msg},
};

mod writer_process;

pub async fn main() {
    let server = stdiomux::mux::basic::server::BasicMuxServer::open(
        BufReader::new(tokio::io::stdin()),
        BufWriter::new(tokio::io::stdout()),
    )
    .await
    .expect("Failed to open server");

    server.run_with(HerderServer {}).await;
    loop {
        let msg = match read_msg_async::<StartHerd<WriteVerifyAction>>(tokio::io::stdin()).await {
            Ok(d) => d,
            Err(e) => {
                tracing::info!("Error received on stdin, quitting: {e}");
                return;
            }
        };
        info!(?msg, "Received StartAction request");

        let child = writer_process::spawn_writer(
            msg.id,
            move |m| {
                write_msg(std::io::stdout(), &(msg.id, TopLevelHerdEvent::from(m))).ok_or_log();
            },
            msg.action,
        );
        info!(?child, "Spawned writer thread");
    }
}

pub struct HerderServer {}

impl HerderServer {
    pub fn new() -> Self {
        Self {}
    }
}

impl<A: HerdAction> HerderService<A> for HerderServer {
    type Error = <A::Event as HerdEvent>::Failure;

    async fn start(
        &mut self,
        action: A,
    ) -> Result<
        crate::herder_api::Started<
            <A::Event as crate::herder_api::HerdEvent>::StartInfo,
            Result<A::Event, Self::Error>,
        >,
        Self::Error,
    > {
        todo!()
    }
}

