//! Extended stream utilities.

use futures::{FutureExt, Stream, StreamExt as _, stream};
use tokio::sync::SetOnce;

/// Flattens [`Result<Stream<Result<T, E>>, E>`] by moving the `E` into the first item of the
/// stream.
pub fn flatten_result_of_stream_of_results<T, E>(
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

impl<T: ?Sized> StreamExt for T where T: Stream {}

pub trait StreamExt: Stream {
    fn inject_err_from_set_once<T, E, E0>(
        self,
        err_notify: impl AsRef<SetOnce<E>>,
    ) -> impl Stream<Item = Result<T, E>>
    where
        Self: Stream<Item = Result<T, E0>> + Sized,
        E: Clone + From<E0>,
    {
        self.map(move |r| {
            let err_notify = err_notify.as_ref();
            match (r.map_err(E::from), err_notify.get()) {
                (_, Some(err)) => Err(err.clone()), // inject error from err_notify
                (Ok(r), None) => Ok(r),
                (Err(e), None) => {
                    // inject error into err_notify
                    err_notify.set(e.clone()).ok();
                    Err(e)
                }
            }
        })
    }

    /// Awaits for the results of the given future. If it results in `Err`, adds an
    /// `Err` onto the stream. If it is `()`, ends the stream without `Err`.
    fn chain_err_from_future<T, E>(
        self,
        future: impl Future<Output = Result<(), E>>,
    ) -> impl Stream<Item = Result<T, E>>
    where
        Self: Stream<Item = Result<T, E>> + Sized,
    {
        let stream_of_just_err = future
            .map(|x| {
                // if x is Err, this will be Some. otherwise it will be None.
                let iter: Option<Result<T, E>> = x.err().map(Err);

                // treat as stream
                stream::iter(iter)
            })
            .flatten_stream();

        self.chain(stream_of_just_err)
    }
}
