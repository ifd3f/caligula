//! Utilities for spawning and interacting with herder daemons.

mod evdist;
mod herder;

pub use herder::{HerderFacade, HerderFacadeImpl, StartWriterError, WriterHandle};
