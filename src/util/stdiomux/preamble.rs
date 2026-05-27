use std::{marker::PhantomData, rc::Rc};

use bytes::Bytes;
use futures::{
    Stream, StreamExt, TryStreamExt,
    stream::{self, LocalBoxStream},
};
use serde::{Serialize, de::DeserializeOwned};

use crate::util::{
    layer_error::LayerError,
    stdiomux::{BytestreamService, service_fn},
    wire::{deserialize, serialize},
};

#[derive(Debug, thiserror::Error)]
pub enum PreambleReadError {
    #[error("Unexpected EOF")]
    UnexpectedClientEof,
    #[error("Deserialization error: {0}")]
    Deserialization(#[from] bincode::Error),
}

#[derive(Debug, thiserror::Error)]
pub enum PreambleWriteError {
    #[error("Serialization error: {0}")]
    Serialization(#[from] bincode::Error),
}

/// Convert the given service into a new one that parses the first payload as
/// the given `P` preamble type, and passes through the rest of the stream to
/// the service `S`.
#[expect(clippy::type_complexity)]
pub fn preamble_request_server<P, S>(
    svc: S,
) -> impl BytestreamService<
    LocalBoxStream<'static, Bytes>,
    Error = LayerError<S::Error, PreambleReadError>,
    Response = LocalBoxStream<'static, Result<Bytes, LayerError<S::Error, PreambleReadError>>>,
> + 'static
where
    P: DeserializeOwned + 'static,
    S: BytestreamService<(P, LocalBoxStream<'static, Bytes>)> + Clone + 'static,
    S::Error: 'static,
{
    service_fn(move |mut req: LocalBoxStream<'static, Bytes>| {
        let svc = svc.clone();
        stream::once(async move {
            let tag = take_req_first::<P>(&mut req)
                .await
                .map_err(LayerError::Transport)?;
            Ok(svc.call((tag, req)).map_err(LayerError::App))
        })
        .map(move_result_into_stream)
        .flatten()
        .boxed_local()
    })
}

/// Convert the given service into a new one that writes the given `P` preamble
/// type as the first payload, and passes through the rest of the stream.
pub fn preamble_request_client<P, S>(svc: S) -> PreambleRequestClient<P, S>
where
    P: Serialize + 'static,
    S: BytestreamService<LocalBoxStream<'static, Bytes>> + 'static,
    S::Response: 'static,
    S::Error: 'static,
{
    PreambleRequestClient {
        svc: Rc::new(svc),
        _phantom: PhantomData,
    }
}

pub struct PreambleRequestClient<P, S> {
    svc: Rc<S>,
    _phantom: PhantomData<fn() -> P>,
}

impl<S, P> BytestreamService<(P, LocalBoxStream<'static, Bytes>)> for PreambleRequestClient<P, S>
where
    P: Serialize + 'static,
    S: BytestreamService<LocalBoxStream<'static, Bytes>> + 'static,
    S::Response: 'static,
    S::Error: 'static,
{
    type Error = LayerError<S::Error, PreambleWriteError>;
    type Response = LocalBoxStream<'static, Result<Bytes, Self::Error>>;

    fn call(&self, (tag, req): (P, LocalBoxStream<'static, Bytes>)) -> Self::Response {
        let this = self.svc.clone();
        stream::once(async move {
            let tag = serialize(&tag);
            let req = stream::once(std::future::ready(tag)).chain(req);
            Ok(this.call(req.boxed_local()).map_err(LayerError::App))
        })
        .map(move_result_into_stream)
        .flatten()
        .boxed_local()
    }
}

/// Take the first thing off a request bytestream and try to deserialize it.
async fn take_req_first<T: DeserializeOwned>(
    req: &mut (impl Stream<Item = Bytes> + Unpin),
) -> Result<T, PreambleReadError> {
    let first_payload = req
        .next()
        .await
        .ok_or(PreambleReadError::UnexpectedClientEof)?;
    let app_req: T = deserialize(&first_payload).map_err(PreambleReadError::Deserialization)?;
    Ok(app_req)
}

/// Convert a `Result<Stream<Result>>` into a `Stream<Result>`.
fn move_result_into_stream<T, E>(
    r: Result<impl Stream<Item = Result<T, E>>, E>,
) -> impl Stream<Item = Result<T, E>> {
    stream::once(std::future::ready(r)).flat_map(|x| {
        let (ok, err) = match x {
            Ok(v) => (Some(v), None),
            Err(e) => (None, Some(Err(e))),
        };
        stream::iter(err).chain(stream::iter(ok).flatten())
    })
}
