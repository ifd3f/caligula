use std::{ffi::OsStr, num::ParseIntError, path::PathBuf};

use bytesize::ByteSize;
use udev::Device;

#[derive(Debug, PartialEq)]
pub struct BurnTarget {
    pub devnode: PathBuf,
    pub size: ByteSize,
    pub model: Option<String>,
    pub removable: Option<bool>,
}

impl PartialOrd for BurnTarget {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        self.devnode.partial_cmp(&other.devnode)
    }
}

impl TryFrom<Device> for BurnTarget {
    type Error = DeviceParseError;

    fn try_from(value: Device) -> Result<Self, Self::Error> {
        if value.subsystem() != Some(OsStr::new("block")) {
            return Err(DeviceParseError::NotABlockDevice);
        }

        let size = if let Some(size) = value.attribute_value("size") {
            let chunks = size.to_string_lossy().parse::<u64>()?;
            ByteSize::b(chunks * 512)
        } else {
            return Err(DeviceParseError::NoSizeAttr);
        };

        let removable = value
            .attribute_value("removable")
            .map(|b| b != OsStr::new("0"));

        let devnode = value
            .devnode()
            .ok_or(DeviceParseError::NoDevNode)?
            .to_owned();

        let model = value
            .attribute_value("device/model")
            .map(|v| v.to_string_lossy().trim().to_owned());

        Ok(Self {
            model,
            removable,
            devnode,
            size,
        })
    }
}

#[derive(Debug, thiserror::Error)]
pub enum DeviceParseError {
    #[error("Not a block device")]
    NotABlockDevice,
    #[error("Could not find path node")]
    NoDevNode,
    #[error("Could not get size")]
    NoSizeAttr,
    #[error("Could not parse size")]
    UnknownSize(#[from] ParseIntError),
}
