use crate::utils::make_hello_with_crate_version;

pub mod client;
pub mod server;

const HELLO: &[u8] = make_hello_with_crate_version!("basic mux");
