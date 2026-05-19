use std::rc::Rc;

use bincode::Options;
use bytes::Bytes;
use futures::{
    Stream, StreamExt, TryStreamExt as _,
    stream::{self, LocalBoxStream},
};

use crate::{
    herder_api::{
        HerderAction, HerderResponse, HerderService, LayerError, bincode_options,
        error::rotate_layer_error,
    },
    stdiomux::{self, BytestreamService},
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

/// Convert a [`HerderService`] server into a [`BytestreamService`] server.
#[expect(clippy::type_complexity)]
pub fn transportize<A, S>(
    svc: S,
) -> impl BytestreamService<
    LocalBoxStream<'static, Bytes>,
    Error = ServerError<S::Error>,
    Response = LocalBoxStream<'static, Result<Bytes, ServerError<S::Error>>>,
>
where
    A: HerderAction,
    S: HerderService<A> + 'static,
    S::Error: 'static,
{
    let svc = Rc::new(svc);
    stdiomux::service_fn(move |req| {
        let svc = svc.clone();
        stream::once(async move {
            let result = handle_request::<A, _>(svc, req).await;
            move_result_into_stream(result)
        })
        .flatten()
        .boxed_local()
    })
}

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

async fn handle_request<A, S>(
    svc: S,
    mut req: impl Stream<Item = Bytes> + Unpin,
) -> Result<impl Stream<Item = Result<Bytes, ServerError<S::Error>>>, ServerError<S::Error>>
where
    A: HerderAction,
    S: HerderService<A>,
{
    let req: A = take_req_first::<A, S::Error>(&mut req).await?;

    #[expect(clippy::type_complexity)]
    let res: Result<HerderResponse<A, S::Error>, LayerError<A::Error, S::Error>> =
        svc.start(req).await;

    let res = serialize_response::<A, S::Error>(res);
    Ok(res.map_err(ServerError::Transport))
}

/// Take the first thing off a request bytestream and try to treat it as
/// [`HerderAction`].
async fn take_req_first<A: HerderAction, Trans>(
    req: &mut (impl Stream<Item = Bytes> + Unpin),
) -> Result<A, ServerError<Trans>> {
    let first_payload = req.next().await.ok_or(ServerError::UnexpectedClientEof)?;
    let app_req: A = bincode_options()
        .deserialize(&first_payload)
        .map_err(ServerError::Deserialization)?;
    Ok(app_req)
}

/// Serialize a response value into Bytes.
fn serialize_response<A: HerderAction, Trans>(
    res: Result<HerderResponse<A, Trans>, LayerError<A::Error, Trans>>,
) -> impl Stream<Item = Result<Bytes, Trans>> + Unpin {
    let (first, rest) = match res {
        Ok(x) => (Ok(x.start), Some(x.events)),
        Err(e) => (Err(e), None),
    };

    let first = rotate_layer_error(first).map(|msg| {
        Bytes::from_owner(
            bincode_options()
                .serialize(&msg)
                .expect("serialization error is impossible"),
        )
    });

    let rest = stream::iter(rest).flat_map(|evs| serialize_events::<A, Trans>(evs));

    stream::once(std::future::ready(first)).chain(rest)
}

/// Serialize an event stream into bytes.
fn serialize_events<A: HerderAction, Trans>(
    res: impl Stream<Item = Result<A::Event, LayerError<A::Error, Trans>>> + Unpin,
) -> impl Stream<Item = Result<Bytes, Trans>> + Unpin {
    res.map(|res| rotate_layer_error(res)).map_ok(|msg| {
        Bytes::from_owner(
            bincode_options()
                .serialize(&msg)
                .expect("serialization error is impossible"),
        )
    })
}
