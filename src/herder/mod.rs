mod handle;
mod herder;
mod socket;

use std::path::PathBuf;
use std::thread::JoinHandle;

pub use handle::WriterHandle;
pub use herder::Herder;
pub use herder::StartWriterError;
pub use socket::HerderSocket;
use tracing::debug;

use crate::writer_process;
use crate::writer_process::ipc::WriterProcessConfig;
