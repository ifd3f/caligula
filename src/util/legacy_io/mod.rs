//! Helpers for I/O and actually interacting with bulk data.
//!
//! This is called legacy_io because I'm about to get rid of this and replace it
//! with a slightly better system.

#[cfg(test)]
mod tests;
mod utils;
mod verify;
mod write;
mod xplat;

pub use utils::SyncDataFile;
pub use verify::VerifyOp;
pub use write::WriteOp;
pub use xplat::open_blockdev;
