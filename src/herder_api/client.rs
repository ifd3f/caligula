use std::marker::PhantomData;

use bytes::Bytes;
use futures::{
    Stream, StreamExt, TryStreamExt,
    stream::{self, LocalBoxStream},
};
use serde::Serialize;
use strum::IntoDiscriminant;

use crate::{
    herder_api::{self, HerderAction, HerderActionResponse, HerderActionService},
    util::{
        layer_error::{LayerError, NestedResultExt as _},
        stdiomux::{BytestreamService, PreambleRequestClient, preamble_request_client},
        wire::{deserialize, serialize},
    },
};

#[derive(Debug, thiserror::Error)]
pub enum ClientError<Trans> {
    #[error("Unexpected Server EOF")]
    UnexpectedServerEof,
    #[error("Transport error: {0}")]
    Transport(Trans),
    #[error("Deserialization error: {0}")]
    Deserialization(#[from] bincode::Error),
}

impl<App, Trans> From<ClientError<Trans>> for LayerError<App, ClientError<Trans>> {
    fn from(value: ClientError<Trans>) -> Self {
        LayerError::Transport(value)
    }
}

/// A client to a remote [`HerderService`] over a transport.
pub struct HerderClient<Req, Svc>
where
    Req: IntoDiscriminant,
    Req::Discriminant: Serialize + 'static,
    Svc: BytestreamService<LocalBoxStream<'static, Bytes>> + 'static,
    Svc::Error: 'static,
{
    client: PreambleRequestClient<Req::Discriminant, Svc>,
    _phantom: PhantomData<fn(Req)>,
}

impl<Req, Svc> HerderClient<Req, Svc>
where
    Req: IntoDiscriminant,
    Req::Discriminant: Serialize + 'static,
    Svc: BytestreamService<LocalBoxStream<'static, Bytes>> + 'static,
    Svc::Error: 'static,
{
    /// Create a [`HerderClient`] over the given transport.
    pub fn new(client: Svc) -> Self {
        Self {
            client: preamble_request_client(client),
            _phantom: PhantomData,
        }
    }
}

impl<Req, A, S> HerderActionService<A> for HerderClient<Req, S>
where
    Req: IntoDiscriminant,
    Req::Discriminant: Serialize + 'static,
    A: Into<Req> + TryFrom<Req> + HerderAction,
    S: BytestreamService<LocalBoxStream<'static, Bytes>> + 'static,
    S::Error: 'static,
{
    type Error = ClientError<S::Error>;

    async fn start(
        &self,
        action: A,
    ) -> Result<
        HerderActionResponse<A, Self::Error>,
        LayerError<<A as herder_api::HerderAction>::Error, Self::Error>,
    > {
        // TODO: figure out how to nicely send the full, tagless req, avoiding the tag
        // preambling dance
        let top_level_req: Req = action.into();
        let tag = top_level_req.discriminant();
        let Ok(action) = A::try_from(top_level_req) else {
            panic!("Req::from or A::try_from implementation is broken")
        };

        let res = self
            .client
            .call((tag, Box::pin(request_into_stream(action))))
            .map_err(|e| e.unwrap_app()); // ignore preamble error

        let (start, events) = handle_response_stream::<A, S::Error>(res).await?;

        Ok(HerderActionResponse {
            start,
            events: Box::pin(events),
        })
    }
}

/// Package a given request into a byte stream.
fn request_into_stream(action: impl HerderAction) -> impl Stream<Item = Bytes> + 'static {
    stream::once(std::future::ready(serialize(&action)))
}

async fn handle_response_stream<A: HerderAction, Trans: 'static>(
    mut res: impl Stream<Item = Result<Bytes, Trans>> + Unpin + 'static,
) -> Result<
    (
        A::Start,
        impl Stream<Item = Result<A::Event, LayerError<A::Error, ClientError<Trans>>>> + 'static,
    ),
    LayerError<A::Error, ClientError<Trans>>,
> {
    // structuring verbosely like this to make it easier to read
    let start: Result<A::Start, LayerError<A::Error, ClientError<Trans>>> =
        take_first::<A, Trans>(&mut res)
            .await
            .flatten_into_layered();
    let start: A::Start = start?;

    let events = stream_into_events::<A, Trans>(Box::pin(res))
        .map(|r: Result<Result<A::Event, A::Error>, ClientError<Trans>>| r.flatten_into_layered());

    Ok((start, events))
}

/// Take the first thing off a response stream and try to treat it as
/// [`HerderAction::Start`] or [`HerderAction::Error`].
async fn take_first<A: HerderAction, Trans>(
    res: &mut (impl Stream<Item = Result<Bytes, Trans>> + Unpin),
) -> Result<Result<A::Start, A::Error>, ClientError<Trans>> {
    let first_result: Result<Bytes, Trans> =
        res.next().await.ok_or(ClientError::UnexpectedServerEof)?;

    let first_payload: Bytes = first_result.map_err(ClientError::Transport)?;

    let first_app_msg: Result<A::Start, A::Error> =
        deserialize(&first_payload).map_err(ClientError::Deserialization)?;

    Ok(first_app_msg)
}

/// Deserialize all remaining messages in a response stream.
fn stream_into_events<A: HerderAction, Trans>(
    res: LocalBoxStream<'static, Result<Bytes, Trans>>,
) -> impl Stream<Item = Result<Result<A::Event, A::Error>, ClientError<Trans>>> {
    res.map_err(ClientError::Transport)
        .and_then(|bs: Bytes| async move {
            let msg: Result<A::Event, A::Error> =
                deserialize(&bs).map_err(ClientError::Deserialization)?;
            Ok(msg)
        })
}
