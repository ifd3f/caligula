use std::sync::{Arc, Mutex};

use bytes::Bytes;

use crate::{mux::channel::shared::WokeRb, util::AnyDrop};

/// Handle to a channel.
pub struct ChannelHandle {
    pub(crate) tx: AnyDrop<Mutex<WokeRb<Bytes>>>,
    pub(crate) rx: Arc<Mutex<WokeRb<Bytes>>>,
}
