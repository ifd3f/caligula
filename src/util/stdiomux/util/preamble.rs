use bincode::Options as _;
use bytes::Bytes;
use futures::{
    Stream, StreamExt, TryStreamExt,
    stream::{self, LocalBoxStream},
};
use serde::{Serialize, de::DeserializeOwned};

use super::{super::StreamService, LayerError, bincode_options, service_fn};

#[derive(Debug, thiserror::Error)]
pub enum TaggedServerError {
    #[error("Unexpected EOF")]
    UnexpectedClientEof,
    #[error("Deserialization error: {0}")]
    Deserialization(#[from] bincode::Error),
}

#[derive(Debug, thiserror::Error)]
pub enum TaggedClientError {
    #[error("Serialization error: {0}")]
    Serialization(#[from] bincode::Error),
}

/// Convert a [`StreamService`] server into a [`StreamService`] over bytes.
#[expect(clippy::type_complexity)]
pub fn preamble_request_server<P, S>(
    svc: S,
) -> impl StreamService<
    LocalBoxStream<'static, Bytes>,
    Error = LayerError<S::Error, TaggedServerError>,
    Response = LocalBoxStream<'static, Result<Bytes, LayerError<S::Error, TaggedServerError>>>,
> + 'static
where
    P: DeserializeOwned + 'static,
    S: StreamService<(P, LocalBoxStream<'static, Bytes>)> + Clone + 'static,
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

/// Convert a [`StreamService`] server into a [`StreamService`] over bytes.
#[expect(clippy::type_complexity)]
pub fn preamble_request_client<P, S>(
    svc: S,
) -> impl StreamService<
    (P, LocalBoxStream<'static, Bytes>),
    Error = LayerError<S::Error, TaggedClientError>,
    Response = LocalBoxStream<'static, Result<Bytes, LayerError<S::Error, TaggedClientError>>>,
> + 'static
where
    P: Serialize + 'static,
    S: StreamService<LocalBoxStream<'static, Bytes>> + Clone + 'static,
    S::Response: 'static,
    S::Error: 'static,
{
    service_fn(move |(tag, req): (P, LocalBoxStream<'static, Bytes>)| {
        let svc = svc.clone();
        stream::once(async move {
            let tag = Bytes::from_owner(
                bincode_options()
                    .serialize(&tag)
                    .map_err(|e| LayerError::Transport(TaggedClientError::Serialization(e)))?
                    .into_boxed_slice(),
            );
            let req = stream::once(std::future::ready(tag)).chain(req);
            Ok(svc.call(req.boxed_local()).map_err(LayerError::App))
        })
        .map(move_result_into_stream)
        .flatten()
        .boxed_local()
    })
}

/// Take the first thing off a request bytestream and try to deserialize it.
async fn take_req_first<T: DeserializeOwned>(
    req: &mut (impl Stream<Item = Bytes> + Unpin),
) -> Result<T, TaggedServerError> {
    let first_payload = req
        .next()
        .await
        .ok_or(TaggedServerError::UnexpectedClientEof)?;
    let app_req: T = bincode_options()
        .deserialize(&first_payload)
        .map_err(TaggedServerError::Deserialization)?;
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
