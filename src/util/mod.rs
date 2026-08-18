//! This directory contains a miscellaneous array of utilities, helpers, and
//! libraries.
//!
//! Generally, there are two kinds of things that belong in here:
//!
//! - Self-contained functions and utilities that are fairly small, but shared
//!   enough that they don't really make much sense elsewhere
//! - Libraries that would theoretically make sense to split into their own
//!   crates later, like [io_graph]
//!
//! As a rule of thumb, if a utility is only used by one subsystem, it should go
//! inside there rather than here. The exception to this is rule those
//! aforementioned large libraries like [io_graph].

use std::{
    alloc::{Layout, alloc},
    env,
    fs::DirBuilder,
    os::unix::fs::DirBuilderExt as _,
    path::PathBuf,
    process,
    time::SystemTime,
};

use bytes::{Bytes, BytesMut};

pub mod byteseries;
pub mod candidate;
pub mod device;
pub mod hyper;
pub mod io_graph;
pub mod legacy_io;
pub mod runtime;

/// Create the directory to shove invocation-specific data into, like log files
/// and sockets.
pub fn ensure_state_dir() -> std::io::Result<PathBuf> {
    let dir = env::temp_dir().join(format!(
        "caligula-{}-{}",
        process::id(),
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_millis(),
    ));

    DirBuilder::new().mode(0o700).recursive(true).create(&dir)?;

    Ok(dir)
}

/// Allocate a [BytesMut] with the given size and uninitialized contents.
///
/// Its capacity and length will both be set to this size.
///
/// This is marked as unsafe because the contents are uninitialized.
pub unsafe fn alloc_uninit_bytes(len: usize) -> BytesMut {
    let mut bytes = BytesMut::with_capacity(len);
    unsafe {
        bytes.set_len(len);
    }
    bytes
}

/// Allocate a [BytesMut] with the size and alignment of the given [Layout] and
/// uninitialized contents.
///
/// Its capacity and length will both be set to the size of the [Layout].
///
/// This is marked as unsafe because the contents are uninitialized.
pub unsafe fn alloc_uninit_bytes_with_layout(layout: Layout) -> BytesMut {
    let len = layout.size();
    let mem = unsafe {
        let ptr = alloc(layout);
        Vec::from_raw_parts(ptr, len, len).into_boxed_slice()
    };
    BytesMut::from(Bytes::from_owner(mem))
}
