//! Module containing common reusable workers.

pub use self::{
    decompress::{DecompressError, DecompressorWorker},
    disk_writer::std::StdDiskWriter,
    file_reader::FileReader,
    hash::HashWorker,
};

mod decompress;
mod disk_writer;
mod file_reader;
mod hash;
