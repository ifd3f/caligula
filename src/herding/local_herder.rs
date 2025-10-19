use std::{collections::BTreeMap, pin::Pin, sync::Arc};

use tokio::{sync::Mutex, task::JoinHandle};
use uuid::Uuid;

use crate::{
    herding::herder::{
        SpawnVerifierError, SpawnVerifierRequest, SpawnVerifierResponse, SpawnWriterError,
        SpawnWriterRequest, SpawnWriterResponse,
    },
    writer::{WriterState, setup_writer},
};

/// A herder for writer processes handled by this process.
struct LocalWriterHerder {
    writers: BTreeMap<Uuid, Child<WriterState>>,
}

/// A herder for writer processes handled by this process.
struct LocalVerifierHerder {
    verifiers: BTreeMap<Uuid, Child<VerifierState>>,
}

struct Child<State> {
    state: Arc<Mutex<State>>,
    join_handle: JoinHandle<()>,
}

impl LocalWriterHerder {
    pub fn new() -> Self {
        Self {
            writers: BTreeMap::new(),
            verifiers: BTreeMap::new(),
        }
    }
}

impl tower::Service<SpawnWriterRequest> for LocalWriterHerder {
    type Response = SpawnWriterResponse;

    type Error = SpawnWriterError;

    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>>>>;

    fn poll_ready(
        &mut self,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        todo!()
    }

    fn call(&mut self, req: SpawnWriterRequest) -> Self::Future {
        async move {
            let (future, state) = setup_writer(req);
            let id = Uuid::new_v4();
            self.writers.insert(
                id,
                Child {
                    state,
                    join_handle: tokio::spawn(future),
                },
            );
            SpawnWriterResponse { id: id }
        }
    }
}

impl tower::Service<SpawnVerifierRequest> for LocalWriterHerder {
    type Response = SpawnVerifierResponse;

    type Error = SpawnVerifierError;

    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>>>>;

    fn poll_ready(
        &mut self,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        std::task::Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: SpawnVerifierRequest) -> Self::Future {
        async move {
            let (future, state) = setup_writer(req);
            let id = Uuid::new_v4();
            self.writers.insert(
                id,
                Child {
                    state,
                    join_handle: tokio::spawn(future),
                },
            );
            SpawnVerifierResponse { id: id }
        }
    }
}

impl super::herder::Herder for LocalWriterHerder {}
