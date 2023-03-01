use std::{borrow::Cow, ffi::OsStr, fmt, num::ParseIntError, path::PathBuf};

use bytesize::ByteSize;
use inquire::Select;
use udev::Device;

pub fn ask_outfile() -> anyhow::Result<PathBuf> {
    let mut enumerator = udev::Enumerator::new()?;
    let devices = enumerator.scan_devices()?;

    let burn_targets = devices.filter_map(|d| BurnTarget::try_from(d).ok());

    //let devs = get_all_device_info(&devpaths)?;
    //println!("{:#?}", devs);

    let removables: Vec<_> = burn_targets.filter(|t| t.removable == Some(true)).collect();

    if removables.is_empty() {
        println!("No removable devices found!");
    } else {
        let ans = Select::new("Select a path", removables);
        ans.prompt()?;
    }
    todo!()
}

#[derive(Debug)]
struct BurnTarget {
    source_device: Device,
    devnode: PathBuf,
    size: ByteSize,
    removable: Option<bool>,
}

impl BurnTarget {
    pub fn model(&self) -> Option<Cow<'_, str>> {
        self.source_device
            .attribute_value("device/model")
            .map(|v| v.to_string_lossy().to_owned())
    }
}

impl fmt::Display for BurnTarget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} ({} ", self.devnode.to_string_lossy(), self.size)?;

        if let Some(m) = self.model() {
            write!(f, "{}", m.trim())?;
        } else {
            write!(f, "Unknown model")?;
        }

        write!(f, ")")?;
        Ok(())
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

        Ok(Self {
            source_device: value,
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
