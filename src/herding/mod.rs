use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use tower::Service;
use uuid::Uuid;

pub use self::worker_factory::{WorkerFactory, WorkerSpawned};

mod local_herder;
mod worker_factory;

/// A [Herder] is a [Service] that spawns and manages a set of workers spawned by a [WorkerFactory].
///
/// Spawning is requested through the [Service] interface.
pub trait Herder<WF>:
    Service<WF::Params, Response = (Uuid, WF::Response), Error = WF::Error>
where
    WF: WorkerFactory,
{
}

/// A batch of status updates from a [Herder]'s workers.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct WorkerUpdateBundle<Report> {
    /// Reports that have changed.
    pub updates: BTreeMap<Uuid, Report>,
    /// Workers that have been removed.
    pub removals: BTreeSet<Uuid>,
}
