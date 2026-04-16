use bincode::Options as _;

use crate::mux::MAX_PAYLOAD;

pub mod mux;

/// Common bincode options to use for inter-process communication.
#[inline]
pub fn bincode_options() -> impl bincode::Options {
    bincode::DefaultOptions::new()
        .with_fixint_encoding()
        .with_big_endian()
        .with_limit(MAX_PAYLOAD as u64)
}
