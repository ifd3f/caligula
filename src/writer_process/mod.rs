//! This module has logic for the child process that writes to the disk.
//!
//! IT IS NOT TO BE USED DIRECTLY BY THE USER! ITS API HAS NO STABILITY GUARANTEES!
mod child;
pub mod ipc;
pub mod state_tracking;
mod xplat;

pub use child::main;
pub use child::spawn;
