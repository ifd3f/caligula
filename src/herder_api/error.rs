use std::{
    error::Error,
    fmt::{Debug, Display},
    io,
};

use serde::{Deserialize, Serialize};
use tracing::debug;

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
    pub fn unwrap_app(self) -> App
    where
        Trans: Debug,
    {
        match self {
            LayerError::App(err) => err,
            LayerError::Transport(err) => panic!(".unwrap_app() failed {err:?}"),
        }
    }

    pub fn map_trans<T2>(self, f: impl FnOnce(Trans) -> T2) -> LayerError<App, T2> {
        match self {
            LayerError::App(err) => LayerError::App(err),
            LayerError::Transport(err) => LayerError::Transport(f(err)),
        }
    }
}

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

/// A serializable [`std::io::Error`] that supports converting it into specific,
/// recognizable kinds of errors.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
pub struct IoError<K: Error> {
    kind: Option<K>,
    raw_error: String,
}

impl<K: Error> IoError<K> {
    pub fn kind(&self) -> Option<&K> {
        self.kind.as_ref()
    }
}

impl<K: Error> Display for IoError<K> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.kind {
            Some(kind) => match f.alternate() {
                true => write!(f, "{kind} (raw error: {}", self.raw_error),
                false => write!(f, "{kind}"),
            },
            None => write!(f, "Unknown I/O error: {}", self.raw_error),
        }
    }
}

impl<K> From<std::io::Error> for IoError<K>
where
    K: Error + TryFrom<std::io::Error>,
{
    fn from(value: std::io::Error) -> Self {
        let raw_error = value.to_string();
        let kind = K::try_from(value).ok();
        Self { kind, raw_error }
    }
}

/// Errors that come up when running one-off shell commands.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
pub enum UnmountError {
    #[error("Error executing diskutil unmount: {0}")]
    Diskutil(CommandError),
}

/// Errors that come up when running one-off shell commands.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
pub enum CommandError {
    #[error("Command exited with exit code {exit:?}!\nstdout: {stdout:?}\nstderr: {stderr:?}")]
    NonzeroExit {
        exit: Option<i32>,
        stdout: String,
        stderr: String,
    },
    #[error("Error managing subprocess: {0}")]
    Process(IoError<UnrecoverableIoError>),
    #[error("Error communicating with subprocess: {0}")]
    Comm(IoError<UnrecoverableIoError>),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
#[error("This error cannot be recovered from!")]
pub struct UnrecoverableIoError;

impl TryFrom<std::io::Error> for UnrecoverableIoError {
    type Error = std::io::Error;

    fn try_from(value: std::io::Error) -> Result<Self, Self::Error> {
        Err(value)
    }
}

/// Errors that come up when opening or reading the input file.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
pub enum InputFileError {
    #[error("Permission denied while opening file!")]
    PermissionDenied,
    #[error("File was not found! Was it moved or deleted?")]
    NotFound,
}

impl TryFrom<std::io::Error> for InputFileError {
    type Error = std::io::Error;

    fn try_from(value: std::io::Error) -> Result<Self, Self::Error> {
        debug!("Attempting to convert std::io::Error into a known value: {value:?}");
        match value.kind() {
            io::ErrorKind::PermissionDenied => Ok(Self::PermissionDenied),
            io::ErrorKind::NotFound => Ok(Self::NotFound),
            _ => Err(value),
        }
    }
}

/// Errors that come up when opening or reading the output file.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
pub enum DiskError {
    #[error("Permission denied while opening file!")]
    PermissionDenied,
    #[error(
        "File is read-only! If we've already become root, this thing simply cannot be written to!"
    )]
    ReadOnly,
    #[error("File was not found! Was it ejected?")]
    NotFound,
}

impl TryFrom<std::io::Error> for DiskError {
    type Error = std::io::Error;

    fn try_from(value: std::io::Error) -> Result<Self, Self::Error> {
        debug!("Attempting to convert std::io::Error into a known value: {value:?}");
        match value.kind() {
            io::ErrorKind::PermissionDenied => Ok(Self::PermissionDenied),
            io::ErrorKind::NotFound => Ok(Self::NotFound),
            io::ErrorKind::ReadOnlyFilesystem => Ok(Self::ReadOnly),
            _ => Err(value),
        }
    }
}
