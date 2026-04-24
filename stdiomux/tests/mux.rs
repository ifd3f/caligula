use std::{
    ops::ControlFlow,
    sync::{Arc, atomic::AtomicBool},
};

use bytes::Bytes;
use stdiomux::{mux::*, test_util::setup_mux_layer_test};

#[tokio::test]
async fn test_send_and_recv() {
    // both sides implicitly send magic handshakes at each other
    let mut harness = setup_mux_layer_test().await;

    let dg = b"foobar";
    harness.aw.send(StreamId(10), dg).await.unwrap();

    let rc = harness.br.recv().await.unwrap();
    assert_eq!(rc, (StreamId(10), Bytes::copy_from_slice(dg)));
}

#[tokio::test]
async fn test_demux() {
    let (dm, mut dmc) = Demux::new();

    let called = Arc::new(AtomicBool::new(false));
    let called_clone = called.clone();
    dmc.set_stream_callback(
        StreamId(10),
        Box::new(move |x: Result<Bytes, Arc<MuxError>>| {
            called_clone.store(true, std::sync::atomic::Ordering::Relaxed);
            assert_eq!(x.unwrap(), Bytes::copy_from_slice(b"foobar"));
            ControlFlow::Continue(())
        }),
    )
    .await;

    dm.handle_datagram(StreamId(10), Ok(Bytes::copy_from_slice(b"foobar")));
}
