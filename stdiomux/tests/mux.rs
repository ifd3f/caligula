use bytes::Bytes;
use stdiomux::{mux::*, test_util::setup_mux_layer_test};

#[tokio::test]
async fn test_send_and_recv() {
    // both sides implicitly send magic handshakes at each other
    let mut harness = setup_mux_layer_test().await;

    let frame = Frame::Data(Bytes::copy_from_slice(b"hello world"));
    harness.aw.sendto(ChannelId(10), &frame).await.unwrap();

    let rc = harness.br.recvfrom().await.unwrap();
    assert_eq!(rc, (ChannelId(10), frame));
}
