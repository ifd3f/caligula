mod file_source_reader;
mod state;
mod util;

use std::sync::Arc;

use tokio::sync::Mutex;

pub use self::state::WriterState;
use crate::herding::herder::SpawnWriterRequest;

/// Maximum size we may allocate for each buffer.
const MAX_BUF_SIZE: usize = 1 << 20; // 1MiB

/// How many bytes should be written before we perform a checkpoint (aka report progress).
const CHECKPOINT_BYTES: usize = 8 * (1 << 20); // 8MiB

pub fn setup_writer(
    params: SpawnWriterRequest,
) -> (impl Future<Output = ()>, Arc<Mutex<WriterState>>) {
}

fn run_writer(state: Arc<Mutex<WriterState>>) {
}
