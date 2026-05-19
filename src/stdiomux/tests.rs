use std::{
    convert::Infallible,
    sync::{Arc, Mutex},
};

use bytes::Bytes;
use futures::{
    StreamExt,
    stream::{self, LocalBoxStream},
};
use proptest::{collection, prelude::*, sample::SizeRange};
use test_strategy::proptest;
use tokio::{io::duplex, runtime::LocalRuntime};
use tracing::{debug, info, info_span};

use crate::stdiomux::{BytestreamService, client, server, service_fn};

/// Happy path testing for an arbitrary set of requests.
#[derive(Debug, Clone)]
struct HappyPathCase(Vec<HappyPathPair>);

#[derive(Debug, Clone)]
struct HappyPathPair {
    /// MAY contain zero-length [Bytes].
    req: Vec<Bytes>,

    /// Guaranteed to NOT contain any zero-length [Bytes].
    expected_res: Vec<Bytes>,
}

fn happy_path_strat(
    payload_size: impl Into<SizeRange>,
    stream_size: impl Into<SizeRange>,
    req_count: impl Into<SizeRange>,
) -> BoxedStrategy<HappyPathCase> {
    let payload_strat = collection::vec(any::<u8>(), payload_size).prop_map(Bytes::from_owner);
    let stream_strat = Arc::new(collection::vec(payload_strat, stream_size));
    let req_res = stream_strat.clone().prop_flat_map(move |req| {
        stream_strat.clone().prop_map(move |mut expected_res| {
            // drop the ones that are zero
            expected_res.retain(|v| !v.is_empty());

            HappyPathPair {
                req: req.clone(),
                expected_res,
            }
        })
    });
    let full_transmission = collection::vec(req_res, req_count);

    full_transmission.prop_map(HappyPathCase).boxed()
}

/// Given a [`HappyPathCase`], returns a [`BytestreamService`] that behaves
/// according to the test case.
fn infallible_service_for_pairs(
    case: HappyPathCase,
) -> Box<
    dyn BytestreamService<
            LocalBoxStream<'static, Bytes>,
            Response = LocalBoxStream<'static, Result<Bytes, Infallible>>,
            Error = Infallible,
        > + Send,
> {
    // we'll work via popping so reverse it first
    let mut pairs = case.0;
    pairs.reverse();

    let pairs = Mutex::new(pairs);
    let svc = service_fn(move |req: LocalBoxStream<'static, Bytes>| {
        let _span = info_span!("infallible_service_for_pairs").entered();

        info!("Got request");

        let mut lock = pairs.lock().unwrap();
        let pair = lock.pop().expect("Got more requests than expected!");
        drop(lock);

        stream::once(async move {
            debug!("Stream is being polled");
            let req = req.collect::<Vec<_>>().await;
            let mut expected_req = pair.req.clone();
            expected_req.retain(|x| !x.is_empty());
            assert_eq!(req, expected_req);

            stream::iter(pair.expected_res).map(Ok::<_, Infallible>)
        })
        .flatten()
        .boxed_local()
    });

    Box::new(svc)
}

#[tokio::test]
async fn test_service_fn() {
    const EXPECTED_REQ: &[&[u8]] = &[b"sam", b"i", b"am"];
    const EXPECTED_RES: &[&[u8]] = &[b"green", b"eggs", b"ham"];

    let service = service_fn(
        move |req: LocalBoxStream<'static, Bytes>| -> LocalBoxStream<'static, Result<Bytes, Infallible>> {
            stream::once(async move {
                let req = req.collect::<Vec<_>>().await;
                assert_eq!(req, EXPECTED_REQ);
                stream::iter(EXPECTED_RES)
                    .map(|x| Ok::<_, Infallible>(Bytes::copy_from_slice(x)))
            })
            .flatten()
            .boxed_local()
        },
    );

    let res = service.call(Box::pin(
        stream::iter(EXPECTED_REQ).map(|b| Bytes::copy_from_slice(b)),
    ));
    let res = res
        .filter_map(|r| std::future::ready(r.ok()))
        .collect::<Vec<_>>()
        .await;

    assert_eq!(res, EXPECTED_RES);
}

#[proptest(async = "tokio")]
async fn proptest_service_fn(
    #[strategy(happy_path_strat(0..100, 0..100, 0..10))] case: HappyPathCase,
) {
    let svc = infallible_service_for_pairs(case.clone());

    for pair in case.0 {
        let res = svc.call(Box::pin(stream::iter(pair.req)));
        let res = res
            .filter_map(|r| std::future::ready(r.ok()))
            .collect::<Vec<_>>()
            .await;

        assert_eq!(res, pair.expected_res);
    }
}

#[proptest]
fn proptest_over_duplex(
    // WARNING: payload_size=0 combined with stream_size=0 is not supported.
    #[strategy(happy_path_strat(1..100, 1..100, 0..10))] case: HappyPathCase,
) {
    tracing_subscriber::fmt::try_init().ok();

    let rt = LocalRuntime::new().unwrap();
    rt.block_on(async move {
        let (c, s) = duplex(65536);

        let (cr, cw) = tokio::io::split(c);
        let (client, client_driver) = client::open(cr, cw);

        let (sr, sw) = tokio::io::split(s);
        let server_driver = server::run(sr, sw, infallible_service_for_pairs(case.clone()));

        let bg =
            tokio::task::spawn_local(async move { tokio::join!(server_driver, client_driver) });

        for pair in case.0 {
            let res = client.call(Box::pin(stream::iter(pair.req)));
            let res = res
                .filter_map(|r| std::future::ready(r.ok()))
                .collect::<Vec<_>>()
                .await;

            assert_eq!(res, pair.expected_res);
        }

        bg.abort();
    });
}
