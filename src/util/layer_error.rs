use std::fmt::Debug;

/// An error that may have happened either in the application, or the transport
/// layer below it.
#[derive(Debug, thiserror::Error)]
pub enum LayerError<App, Trans> {
    #[error("Application error: {0}")]
    App(App),
    #[error("Transport error: {0}")]
    Transport(Trans),
}

impl<App, Trans> LayerError<App, Trans> {
    #[expect(unused)]
    pub fn unwrap_app(self) -> App
    where
        Trans: Debug,
    {
        match self {
            LayerError::App(err) => err,
            LayerError::Transport(err) => panic!(".unwrap_app() failed {err:?}"),
        }
    }

    #[expect(unused)]
    pub fn map_trans<T2>(self, f: impl FnOnce(Trans) -> T2) -> LayerError<App, T2> {
        match self {
            LayerError::App(err) => LayerError::App(err),
            LayerError::Transport(err) => LayerError::Transport(f(err)),
        }
    }
}

#[expect(unused)]
pub trait NestedResultExt<T, App, Trans> {
    /// Unnest this [`Result`] by rotating the inner error into a
    /// [`LayerError`].
    fn flatten_into_layered(self) -> Result<T, LayerError<App, Trans>>;
}

impl<T, App, Trans> NestedResultExt<T, App, Trans> for Result<Result<T, App>, Trans> {
    fn flatten_into_layered(self) -> Result<T, LayerError<App, Trans>> {
        match self {
            Ok(Ok(x)) => Ok(x),
            Ok(Err(app)) => Err(LayerError::App(app)),
            Err(trans) => Err(LayerError::Transport(trans)),
        }
    }
}

pub trait LayerResultExt<T, App, Trans> {
    /// Eliminate this [`Result`]'s [`LayerError`] by rotating the
    /// application-level errors into the [`Ok`].
    fn rotate_into_ok(self) -> Result<Result<T, App>, Trans>;
}

impl<T, App, Trans> LayerResultExt<T, App, Trans> for Result<T, LayerError<App, Trans>> {
    fn rotate_into_ok(self) -> Result<Result<T, App>, Trans> {
        match self {
            Ok(x) => Ok(Ok(x)),
            Err(LayerError::App(app)) => Ok(Err(app)),
            Err(LayerError::Transport(trans)) => Err(trans),
        }
    }
}
