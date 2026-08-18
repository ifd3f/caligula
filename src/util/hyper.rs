use std::marker::PhantomData;

#[derive(Clone)]
pub struct TokioLocalExecutor {
    _private: (),
}

impl TokioLocalExecutor {
    pub fn new() -> Self {
        Self { _private: () }
    }
}

impl<F> hyper::rt::Executor<F> for TokioLocalExecutor
where
    F: Future + 'static,
    F::Output: 'static,
{
    fn execute(&self, future: F) {
        tokio::task::spawn_local(future);
    }
}
