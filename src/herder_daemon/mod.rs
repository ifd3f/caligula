//! This module contains the herder daemon process, along with all of the
//! utilities it uses to herd and monitor groups of threads.

// Side note: Interestingly, this interface can theoretically be used to have
// caligula delegate writing to remote hosts over SSH. This may be a very
// strange but funny feature to implement.

use std::convert::Infallible;

use bytes::Bytes;
use futures::TryStreamExt;
use http::{Method, StatusCode};
use http_body_util::{BodyExt, BodyStream, combinators::BoxBody};
use hyper::{body, service::service_fn};
use hyper_util::{
    rt::{TokioExecutor, TokioIo},
    service::TowerToHyperService,
};
use tokio::sync::{mpsc, oneshot};
use tokio_stream::wrappers::UnboundedReceiverStream;
use tower::ServiceExt;
use tracing::{debug, info};

use crate::{
    herder_api::{
        HerderResponse, HerderService,
        error::LayerError,
        server::transportize,
        write_verify::{WVAction, WVError},
    },
    util::{
        hyper::TokioLocalExecutor,
        runtime::{AsyncRuntime, RemoteSpawn as _},
    },
};

mod writer_process;

pub fn main() {
    AsyncRuntime::start()
        .spawn(|| {
            hyper::server::conn::http2::Builder::new(TokioLocalExecutor::new())
                .keep_alive_interval(None)
                .serve_connection(
                    TokioIo::new(tokio::io::join(tokio::io::stdin(), tokio::io::stdout())),
                    TowerToHyperService::new(tower::service_fn(handler)),
                )
        })
        .blocking_recv()
        .expect("Daemon dropped!");
}

async fn handler(
    req: http::Request<body::Incoming>,
    hs: HerderServer,
) -> hyper::Response<BoxBody<Bytes, Infallible>> {
    match req.uri().path() {
        "/actions/write_verify" => action_handler(req, service_fn(|r| {})).await,
        _ => {
            let mut r = http::Response::new(BoxBody::new(b"not found"));
            *r.status_mut() = StatusCode::NOT_FOUND;
            r
        }
    }
}

async fn action_handler(
    req: http::Request<body::Incoming>,
    s: impl tower::Service<body::Incoming, Response = BoxBody<Bytes, Infallible>, Error = Infallible>,
) -> http::Response<BoxBody<Bytes, Infallible>> {
    match *req.method() {
        Method::POST => {
            let body = s.oneshot(req.into_body()).await.unwrap();

            let mut r = http::Response::new(body);
            *r.status_mut() = StatusCode::OK;
            r
        }
        _ => {
            let mut r = http::Response::new(BoxBody::new(b"method not allowed"));
            *r.status_mut() = StatusCode::METHOD_NOT_ALLOWED;
            r
        }
    }
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
