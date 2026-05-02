mod traits;

#[cfg(feature = "io-tokio")]
pub mod tokio;
mod util;

pub use traits::{ChannelHandle, MuxController};
pub use util::ChannelHandleExt;
