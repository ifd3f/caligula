use bytes::Bytes;
use futures::{
    Stream, StreamExt, TryStreamExt,
    stream::{self, BoxStream, LocalBoxStream},
};

use crate::{
    herder_api::{HerderAction, HerderResponse, HerderService},
    util::{
        layer_error::LayerError,
        stdiomux::BytestreamService,
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
/// A client to a remote [`HerderService`] over a transport. Created using the
/// [`create()`] function.
///
/// Technically speaking, it only supports one request right now, and explodes
/// afterwards, but that's okay! Refactors will come Soon(tm).
pub struct HerderClient<S> {
    client: S,
}

impl<S> HerderClient<S>
where
    S: BytestreamService<BoxStream<'static, Bytes>>,
    S::Response: Unpin + 'static,
    S::Error: 'static,
{
    /// Create a [`HerderClient`] over the given transport.
    pub fn new(client: S) -> Self {
        Self { client }
    }
}

impl<A, S> HerderService<A> for HerderClient<S>
where
    A: HerderAction,
    S: BytestreamService<BoxStream<'static, Bytes>>,
    S::Response: Unpin + 'static,
    S::Error: 'static,
{
    type Error = ClientError<S::Error>;

    async fn start(
        &self,
        action: A,
    ) -> Result<HerderResponse<A, Self::Error>, LayerError<A::Error, Self::Error>> {
        // TODO: implement multiplexing
        let req = request_into_stream(action);

        let mut res = self.client.call(req);

        let start = take_first::<A, _>(&mut res).await?;
        let events = stream_into_events::<A, _>(Box::pin(res));

        Ok(HerderResponse {
            start,
            events: Box::pin(events),
        })
    }
}

/// Package a given request into a byte stream.
fn request_into_stream(action: impl HerderAction) -> BoxStream<'static, Bytes> {
    Box::pin(stream::once(std::future::ready(serialize(&action))))
}

/// Take the first thing off a response stream and try to treat it as
/// [`HerderAction::Start`] or [`HerderAction::Error`].
async fn take_first<A: HerderAction, Trans>(
    res: &mut (impl Stream<Item = Result<Bytes, Trans>> + Unpin),
) -> Result<A::Start, LayerError<A::Error, ClientError<Trans>>> {
    let first_result = res.next().await.ok_or(ClientError::UnexpectedServerEof)?;
    let first_payload = first_result.map_err(ClientError::Transport)?;
    let first_app_msg: Result<A::Start, A::Error> =
        deserialize(&first_payload).map_err(ClientError::Deserialization)?;
    let start = first_app_msg.map_err(LayerError::App)?;
    Ok(start)
}

/// Deserialize all remaining messages in a response stream.
fn stream_into_events<A: HerderAction, Trans>(
    res: LocalBoxStream<'static, Result<Bytes, Trans>>,
) -> impl Stream<Item = Result<A::Event, LayerError<A::Error, ClientError<Trans>>>> {
    res.map_err(ClientError::Transport).map(|res| {
        let bs = res.map_err(LayerError::Transport)?;
        let msg: Result<A::Event, A::Error> = deserialize(bs)
            .map_err(ClientError::Deserialization)
            .map_err(LayerError::Transport)?;
        msg.map_err(LayerError::App)
    })
}
