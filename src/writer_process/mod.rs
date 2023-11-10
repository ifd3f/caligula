//! This module has logic for the child process that writes to the disk.
//!
//! IT IS NOT TO BE USED DIRECTLY BY THE USER! ITS API HAS NO STABILITY GUARANTEES!
pub mod child;
pub mod handle;
pub mod ipc;
pub mod state_tracking;
mod xplat;

pub use handle::Handle;
