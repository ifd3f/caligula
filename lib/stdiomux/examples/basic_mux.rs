use std::{
    convert::Infallible,
    task::{Context, Poll},
};

use bytes::Bytes;
use futures::{
    Stream, StreamExt,
    future::{BoxFuture, poll_fn},
    stream,
};
use stdiomux::mux::{
    ByteStream,
    basic::{client::open_client, server::BasicMuxServer},
};
use tower_service::Service;

#[tokio::main]
async fn main() {
    let (c2sr, c2sw) = tokio_pipe::pipe().unwrap();
    let (s2cr, s2cw) = tokio_pipe::pipe().unwrap();

    let c = tokio::spawn(async move {
        let (mut c, d) = open_client(s2cr, c2sw)
            .await
            .expect("failed to open client");
        tokio::spawn(d);

        loop {
            poll_fn(|cx| c.poll_ready(cx))
                .await
                .expect("expected client to be up");
            let request = vec![Bytes::from_static(b"foobar"), Bytes::from_static(b"spam")];
            println!("sending request {request:?}");
            let response = c
                .call(Box::pin(stream::iter(request)))
                .await
                .expect("should be successful");

            let response: Vec<Bytes> = response.collect().await;
            println!("got response {response:?}");
        }
    });

    let s = tokio::spawn(async move {
        BasicMuxServer::open(c2sr, s2cw)
            .await
            .expect("server failed to open")
            .run_with(EchoServer)
            .await
            .expect("server error")
    });

    s.await.unwrap();
    c.await.unwrap();
}

struct EchoServer;

impl<Req> Service<Req> for EchoServer
where
    Req: Stream<Item = Bytes> + Unpin + Send + 'static,
{
    type Response = ByteStream;

    type Error = Infallible;

    type Future = BoxFuture<'static, Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: Req) -> Self::Future {
        Box::pin(async move {
            let res: ByteStream = Box::pin(stream::unfold(req, |mut req| async move {
                let next = req.next().await?;
                Some((next, req))
            }));
            Ok(res)
        })
    }
}
