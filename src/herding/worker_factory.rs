use std::fmt::Debug;
use std::sync::Arc;

use futures::future::BoxFuture;
use serde::{Serialize, de::DeserializeOwned};

/// Spawns workers and provides a function to report on their statuses.
///
/// Worker lifecycle is managed by Herders.
pub trait WorkerFactory: Sync + Send {
    /// Parameters for spawning the worker.
    type Params: Serialize + DeserializeOwned;
    /// Response for a successfully-spawned worker.
    type Response: Serialize + DeserializeOwned;
    /// Error for a unsuccessfully-spawned worker.
    type Error: Serialize + DeserializeOwned;

    /// State for the owning [Herder] to manage.
    type State: Sync + Send + 'static;
    /// A version of [`Self::State`] that can be serialized onto a wire.
    type Report: Serialize + DeserializeOwned + PartialEq + Clone + Debug + Send + Sync + 'static;

    /// Attempt to spawn a worker.
    async fn spawn(
        &self,
        r: Self::Params,
    ) -> Result<WorkerSpawned<Self::Response, Self::State, BoxFuture<'static, ()>>, Self::Error>;

    /// Transform worker state into a reportable value.
    fn report(&self, s: &Self::State) -> Self::Report;
}

/// Result of a successfully spawned worker.
pub struct WorkerSpawned<R, S, F> {
    /// The response to send to the caller.
    pub response: R,
    /// The state for the owning [Herder] to manage.
    pub state: Arc<S>,
    /// A [Future] representing completion of the worker.
    pub future: F,
}
