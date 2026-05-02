use rstest::rstest;
use stdiomux::test_util::mux_harness::{SimpleMuxFactory, TestControllerFactory, new_test_harness};

#[rstest]
#[tokio::test]
async fn open_works(#[values(SimpleMuxFactory)] f: impl TestControllerFactory) {
    new_test_harness(&f, &f).await;
}

            #[test_strategy::proptest]
async fn open_channel(#[values(SimpleMuxFactory)] f: impl TestControllerFactory, ) {
    new_test_harness(&f, &f).await.run_with(|c, _| async move {

    }, |c, _| async move{

    }).await;
}
