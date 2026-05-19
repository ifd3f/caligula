#![expect(unused, reason = "Stub interface created for later use.")]

use std::{collections::VecDeque, iter, sync::Mutex};

use crate::util::device::{DeviceParseError, WriteTarget};

/// Handle for viewing the list of disks we've found.
pub struct DiskList {
    loading: bool,

    /// List of disks we've discovered.
    disks: Vec<WriteTarget>,

    /// Errors encountered while listing disks.
    errors: Mutex<VecDeque<DeviceParseError>>,
}

impl DiskList {
    /// Returns true if we're still loading the initial list of disks.
    pub fn loading(&self) -> bool {
        self.loading
    }

    /// List of disks we've discovered.
    pub fn disks(&self) -> &[WriteTarget] {
        &self.disks
    }

    /// All errors encountered while listing disks, in order as encountered.
    ///
    /// Accessing this iterator will pop errors off this disk list.
    pub fn errors(&self) -> impl Iterator<Item = DeviceParseError> + '_ {
        let mut guard = self.errors.lock().unwrap();
        iter::from_fn(move || guard.pop_front())
    }
}
