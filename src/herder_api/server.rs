use std::{collections::HashMap, convert::Infallible, rc::Rc};

use bincode::Options;
use bytes::Bytes;
use futures::{Stream, StreamExt, TryStreamExt as _};
use http_body_util::{BodyExt, combinators::BoxBody};
use hyper::body;
use tracing_unwrap::ResultExt;

use crate::herder_api::{
    HerderAction, HerderResponse, HerderService, LayerError, bincode_options,
    error::rotate_layer_error,
};

#[derive(Debug, thiserror::Error)]
pub enum ServerError<Trans> {
    #[error("Unexpected EOF")]
    UnexpectedClientEof,
    #[error("Transport error: {0}")]
    Transport(Trans),
    #[error("Deserialization error: {0}")]
    Deserialization(#[from] bincode::Error),
}

/// Convert a [`HerderService`] server into a [`tower::Service`]
/// that takes in a [`body::Incoming`] and spits out another body
#[expect(clippy::type_complexity)]
pub fn transportize<A, S>(
    svc: S,
) -> impl tower::Service<
    body::Incoming,
    Error = ServerError<S::Error>,
    Response = http::Response<BoxBody<Bytes, Infallible>>,
>
where
    A: HerderAction,
    S: HerderService<A> + 'static,
    S::Error: 'static,
{
    let svc = Rc::new(svc);
    tower::service_fn(move |req: body::Incoming| {
        let svc = svc.clone();
        async move {
            let x = req.collect().await.unwrap();
            let Some(x) = bincode_options()
                .deserialize::<A>(&x.to_bytes())
                .ok_or_log()
            else {
                return Ok({
                    let mut r = http::Response::new(BoxBody::new(b"bad request"));
                    *r.status_mut() = http::StatusCode::BAD_REQUEST;
                    r
                });
            };

            let res = svc.start(x).await;

            match res {
                Ok(x) => {
                    let x = bincode_options().serialize(&x.start).unwrap();
                    x
                }
                Err(LayerError::App(e)) => Ok({
                    let body = BoxBody::new(bincode_options().serialize(&e).unwrap());
                    let mut r = http::Response::new(body);
                    *r.status_mut() = http::StatusCode::SERVICE_UNAVAILABLE;
                    r
                }),
                Err(LayerError::Transport(e)) => Err(ServerError::Transport(e)),
            }
        }
    })
}
