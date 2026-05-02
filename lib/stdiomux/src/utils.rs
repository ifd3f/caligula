/// Generates a [`libc::PIPE_BUF`]-length string with the crate's version in it, suitable for validating handshakes.
macro_rules! make_hello_with_crate_version {
    ($controller_name:expr) => {{
        const START: &'static [u8] = concat!(
            env!("CARGO_CRATE_NAME"),
            " ",
            env!("CARGO_PKG_VERSION"),
            " ",
            $controller_name
        )
        .as_bytes();
        ::byte_strings::const_concat_bytes!(START, &[0u8; ::libc::PIPE_BUF - START.len()])
    }};
}
pub(crate) use make_hello_with_crate_version;
