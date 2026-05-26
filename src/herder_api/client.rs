use bincode::Options as _;
use bytes::Bytes;
use futures::{
    Stream, StreamExt, TryStreamExt,
    stream::{self, BoxStream, LocalBoxStream},
};

use crate::{
    herder_api::{HerderAction, HerderActionResponse, HerderActionService, bincode_options},
    util::stdiomux::{
        StreamService,
        util::{LayerError, preamble_request_client, preamble_request_server},
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
    S: StreamService<LocalBoxStream<'static, Bytes>>,
    S::Response: Unpin + 'static,
    S::Error: 'static,
{
    /// Create a [`HerderClient`] over the given transport.
    pub fn new(client: S) -> Self {
        Self { client }
    }
}

impl<A, S> HerderActionService<A> for HerderClient<S>
where
    A: HerderAction,
    S: StreamService<LocalBoxStream<'static, Bytes>>,
    S::Response: Unpin + 'static,
    S::Error: 'static,
{
    type Error = ClientError<S::Error>;

    async fn start(
        &self,
        action: A,
    ) -> Result<HerderActionResponse<A, Self::Error>, LayerError<A::Error, Self::Error>> {
        let s = preamble_request_client::<A, _>(&self.client);
        let mut res = s.call((action, stream::empty().boxed_local()));

        let start = take_first::<A, _>(&mut res).await?;
        let events = stream_into_events::<A, _>(Box::pin(res));

        Ok(HerderActionResponse {
            start,
            events: Box::pin(events),
        })
    }
}

/// Package a given request into a byte stream.
fn request_into_stream(action: impl HerderAction) -> BoxStream<'static, Bytes> {
    let msg = Bytes::from_owner(
        bincode_options()
            .serialize(&action)
            .expect("Serialization error is impossible"),
    );
    Box::pin(stream::once(std::future::ready(msg)))
}

/// Take the first thing off a response stream and try to treat it as
/// [`HerderAction::Start`] or [`HerderAction::Error`].
async fn take_first<A: HerderAction, Trans>(
    res: &mut (impl Stream<Item = Result<Bytes, Trans>> + Unpin),
) -> Result<A::Start, LayerError<A::Error, ClientError<Trans>>> {
    let first_result = res.next().await.ok_or(ClientError::UnexpectedServerEof)?;
    let first_payload = first_result.map_err(ClientError::Transport)?;
    let first_app_msg: Result<A::Start, A::Error> = bincode_options()
        .deserialize(&first_payload)
        .map_err(ClientError::Deserialization)?;
    let start = first_app_msg.map_err(LayerError::App)?;
    Ok(start)
}

/// Deserialize all remaining messages in a response stream.
fn stream_into_events<A: HerderAction, Trans>(
    res: LocalBoxStream<'static, Result<Bytes, Trans>>,
) -> impl Stream<Item = Result<A::Event, LayerError<A::Error, ClientError<Trans>>>> {
    res.map_err(ClientError::Transport).map(|res| {
        let bs = res.map_err(LayerError::Transport)?;
        let msg: Result<A::Event, A::Error> = bincode_options()
            .deserialize(&bs)
            .map_err(ClientError::Deserialization)
            .map_err(LayerError::Transport)?;
        msg.map_err(LayerError::App)
    })
}
