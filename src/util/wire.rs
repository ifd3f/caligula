use bincode::Options as _;
use bytes::Bytes;
use serde::{Serialize, de::DeserializeOwned};

pub fn deserialize<T: DeserializeOwned>(b: impl AsRef<[u8]>) -> Result<T, bincode::Error> {
    bincode_options().deserialize(b.as_ref())
}

pub fn serialize<T: Serialize>(x: &T) -> Bytes {
    Bytes::from_owner(
        bincode_options()
            .serialize(&x)
            .expect("Serialization error is impossible")
            .into_boxed_slice(),
    )
}

/// Common bincode options to use for inter-process communication.
#[inline]
fn bincode_options() -> impl bincode::Options {
    bincode::DefaultOptions::new()
        .with_fixint_encoding()
        .with_native_endian()
        .with_limit(1024)
}
