#![expect(unused, reason = "Stub interface created for later use.")]

use std::path::PathBuf;

use bytes::Bytes;

use crate::{
    codec::{compression::CompressionFormat, hash::HashAlg},
    util::candidate::Candidates,
};

/// Result of analyzing an input file for its properties.
pub struct FileAnalysis {
    pub compression: Candidates<CompressionCandidate>,
    pub hash_file: Candidates<HashFile>,
}

/// Newtype wrapper around [`CompressionFormat`] that supports [`Ord`].
///
/// All compression algorithms are considered equal for [`Candidates`] purposes.
#[derive(Debug)]
pub struct CompressionCandidate(pub CompressionFormat);

impl PartialEq for CompressionCandidate {
    fn eq(&self, other: &Self) -> bool {
        true
    }
}

impl Eq for CompressionCandidate {}

impl PartialOrd for CompressionCandidate {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for CompressionCandidate {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        std::cmp::Ordering::Equal
    }
}

/// Hash value derived from a file.
///
/// In case of tie, the more secure algorithms win.
#[derive(Debug)]
pub struct HashFile {
    pub alg: HashAlg,
    /// The hash we expect.
    pub expected_hash: Bytes,
    /// Name of the file
    pub file: PathBuf,
}

impl PartialEq for HashFile {
    fn eq(&self, other: &Self) -> bool {
        self.alg.eq(&other.alg)
    }
}

impl Eq for HashFile {}

impl PartialOrd for HashFile {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for HashFile {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.alg.cmp(&other.alg)
    }
}
