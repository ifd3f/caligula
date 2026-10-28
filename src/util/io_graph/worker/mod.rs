//! Module containing common reusable workers.

pub use self::{
    decompress::{DecompressError, DecompressorWorker},
    file_reader::FileReader,
    file_writer::FileWriter,
    hash::HashWorker,
};

mod decompress;
mod file_reader;
mod file_writer;
mod hash;
