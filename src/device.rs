use std::{
    ffi::OsStr,
    io,
    num::ParseIntError,
    path::{Path, PathBuf},
};

use bytesize::ByteSize;
use udev::Device;

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct BurnTarget {
    pub devnode: PathBuf,
    pub size: Option<ByteSize>,
    pub model: Option<String>,
    pub removable: Option<bool>,
    pub target_type: Type,
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum Type {
    File,
    Block,
}

impl PartialOrd for BurnTarget {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        self.devnode.partial_cmp(&other.devnode)
    }
}

impl TryFrom<&Path> for BurnTarget {
    type Error = DeviceParseError;

    fn try_from(value: &Path) -> Result<Self, Self::Error> {
        let mut enumerator = udev::Enumerator::new()?;
        let devices = enumerator.scan_devices()?;

        let udev = devices.filter(|d| d.devnode() == Some(value)).next();
        if let Some(udev) = udev {
            return BurnTarget::try_from(udev);
        }

        Ok(BurnTarget {
            devnode: value.to_owned(),
            size: None,
            model: None,
            removable: None,
            target_type: Type::File,
        })
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
            Some(ByteSize::b(chunks * 512))
        } else {
            None
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
            target_type: Type::Block,
        })
    }
}

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum DeviceParseError {
    #[error("Not a block device")]
    NotABlockDevice,
    #[error("Could not find path node")]
    NoDevNode,
    #[error("Could not parse size")]
    UnknownSize(#[from] ParseIntError),
    #[error("Udev error:")]
    Udev(#[from] io::Error),
}
