//! Module containing common reusable workers.

pub use self::{decompress::DecompressorWorker, file_reader::FileReader, hash::HashWorker};

mod decompress;
mod file_reader;
mod hash;
