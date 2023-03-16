///! This module has logic for the child process.
///! IT IS NOT TO BE USED DIRECTLY BY THE USER!
///! ITS UI IS COMPLETELY UNSTABLE!
pub mod child;
pub mod handle;
pub mod ipc;
pub mod state_tracking;
mod xplat;

pub const BURN_ENV: &str = "_CALIGULA_RUN_IN_BURN_MODE";

pub use handle::Handle;
