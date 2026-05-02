use tokio::sync::Barrier;
use tokio_pipe::{PipeRead, PipeWrite};

use crate::{
    mux::{MuxController, tokio::SimpleMuxController},
    test_util::kill_pipe::{DuplexKillController, KillRead, KillWrite, duplex_kill_pipe},
};

pub struct TestHarness<C: MuxController> {
    /// Mux A
    pub a: C,
    /// Mux B
    pub b: C,
    /// Controls for cancelling them
    pub cancel: DuplexKillController,
}

pub struct Shared {
    pub barrier: Barrier,
}

impl<C: MuxController> TestHarness<C> {
    pub async fn run_with<FA, FB>(
        self,
        a: impl FnOnce(C, &Shared) -> FA,
        b: impl FnOnce(C, &Shared) -> FB,
    ) where
        FA: Future<Output = ()>,
        FB: Future<Output = ()>,
    {
        let s = Shared {
            barrier: Barrier::new(2),
        };
        tokio::join!(a(self.a, &s), b(self.b, &s));
    }
}

pub trait TestControllerFactory {
    type Output: MuxController;

    /// Create a [MuxController] connected to the provided [AsyncRead] and [AsyncWrite].
    fn create(
        &self,
        r: KillRead<PipeRead>,
        w: KillWrite<PipeWrite>,
    ) -> impl Future<Output = Self::Output>;
}

pub async fn new_test_harness<F: TestControllerFactory>(a: &F, b: &F) -> TestHarness<F::Output> {
    let (cancel, pipes) = duplex_kill_pipe().unwrap();
    let (a, b) = tokio::join!(
        a.create(pipes.b2ar, pipes.a2bw),
        b.create(pipes.a2br, pipes.b2aw)
    );
    TestHarness { a, b, cancel }
}

pub struct SimpleMuxFactory;

impl TestControllerFactory for SimpleMuxFactory {
    type Output = SimpleMuxController<KillRead<PipeRead>, KillWrite<PipeWrite>>;

    fn create(
        &self,
        r: KillRead<PipeRead>,
        w: KillWrite<PipeWrite>,
    ) -> impl Future<Output = Self::Output> {
        async move { SimpleMuxController::open(r, w).await.unwrap() }
    }
}
