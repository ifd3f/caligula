//! This module contains the herder daemon process, along with all of the utilities it uses to
//! herd and monitor groups of threads.

// Side note: Interestingly, this interface can theoretically be used to have caligula delegate
// writing to remote hosts over SSH. This may be a very strange but funny feature to implement.

use bytes::Bytes;
use http::Response;
use tower::Service;
use std::{convert::Infallible, task::{Context, Poll}};

use futures::{Stream, future::BoxFuture, stream::BoxStream};
use http_body_util::StreamBody;
use hyper::{Request, body::{self, Body}, server};
use hyper_util::{rt::{TokioExecutor, TokioIo}, service::TowerToHyperService};
use tokio::{io::DuplexStream, runtime::Handle};
use tracing::info;
use tracing_unwrap::ResultExt;

use crate::{
    herder_daemon::ipc::{TopLevelHerdEvent, WriteVerifyAction},
    ipc_common::{read_msg_async, write_msg},
};

pub mod ipc;
mod writer_process;

pub async fn main() {
    loop {
        let msg =
            match read_msg_async::<ipc::StartHerd<WriteVerifyAction>>(tokio::io::stdin()).await {
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

        server::conn::http2::Builder::new(TokioExecutor::new())
            .keep_alive_interval(None)
            .serve_connection(
                TokioIo::new(tokio_duplex::Duplex::new(
                    tokio::io::stdin(),
                    tokio::io::stdout(),
                )),
                TowerToHyperService ::new(MyService{})
            ).await.unwrap();
    }
}

#[derive(Clone)]
struct MyService {

}


impl Service<Request<hyper::body::Incoming>> for MyService {
    type Response = Response<StreamBody<BoxStream<'static, Result<hyper::body::Frame<Bytes>, Infallible>>>>;
    type Error = Infallible;
    type Future = std::pin::Pin<Box<dyn std::future::Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn call(&mut self, req: Request<hyper::body::Incoming>) -> Self::Future {
        std::hint::black_box(todo!())
    }
    
    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        todo!()
    }
}