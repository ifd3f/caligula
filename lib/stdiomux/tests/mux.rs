use std::usize;

use proptest::prelude::*;
use stdiomux::{
    mux::{
        ByteStream,
        basic::{client, server::BasicMuxServer},
    },
    test_util::{
        kill_pipe::duplex_kill_pipe,
        mux::{
            action::{ChannelAction, SidedAction},
            harness::test_single_channel,
            strategy::random_channel_strat,
        },
    },
};
use tower::ServiceExt;

#[test_strategy::proptest(
    ProptestConfig {
        timeout: 10,
        .. ProptestConfig::default()
    },
    async = "tokio"
)]
#[test_log::test]
async fn basic_mux_works_single_channel(
    #[strategy(random_channel_strat(0..10, 1..24, usize::MAX))] actions: Vec<
        SidedAction<ChannelAction>,
    >,
) {
    let (_controller, pipes) = duplex_kill_pipe().unwrap();

    // open client and server in parallel
    let client = client::open(pipes.a2br, pipes.b2aw);
    let server = BasicMuxServer::open(pipes.b2ar, pipes.a2bw);
    let (client, server) = tokio::join!(client, server);
    let ((client, drive_client), server) = (client.unwrap(), server.unwrap());

    // drive server in background
    let drcl = tokio::spawn(drive_client);

    // run the actual test
    test_single_channel(
        client.map_response(|r| -> ByteStream { Box::pin(r) }),
        move |s| async move {
            server
                .run_with(s.clone().map_request(|r| Box::pin(r)))
                .await
                .unwrap();
        },
        actions,
    )
    .await;

    drcl.abort();
}
