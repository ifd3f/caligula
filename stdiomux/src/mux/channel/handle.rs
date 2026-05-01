use std::sync::Arc;

use super::shared::Shared;

/// Handle to a channel.
pub struct ChannelHandle {
    /// User owns this.
    pub(crate) shared: Arc<Shared>,
}
