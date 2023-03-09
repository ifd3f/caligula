use std::{
    ffi::OsStr,
    fmt::Display,
    io,
    path::{Path, PathBuf},
};

use bytesize::ByteSize;
use serde::{Deserialize, Serialize};
use valuable::Valuable;

#[cfg(target_os = "linux")]
pub fn enumerate_devices() -> impl Iterator<Item = BurnTarget> {
    use std::fs::read_dir;

    let paths = read_dir("/sys/class/block").unwrap();

    paths
        .filter_map(|r| r.ok())
        .filter_map(|d| BurnTarget::try_from(d.path().as_ref()).ok())
}

#[cfg(target_os = "macos")]
pub fn enumerate_devices() -> impl Iterator<Item = BurnTarget> {
    use std::{
        ffi::{CStr, OsString},
        os::unix::prelude::OsStrExt,
    };

    use libc::{c_void, free};

    use crate::native::{self, enumerate_disks};

    let mut out = Vec::new();

    unsafe {
        let list = enumerate_disks();

        for i in 0..list.n {
            let d = *list.disks.offset(i as isize);
            let bsdname = OsStr::from_bytes(CStr::from_ptr(d.bsdname).to_bytes());
            let mut rawdevname = OsString::from("r");
            rawdevname.push(bsdname);
            let devnode: PathBuf = PathBuf::from("/dev").join(rawdevname);
            let bsdname = bsdname.to_string_lossy().into();
            free(d.bsdname as *mut c_void);

            let model = Model(if d.model.is_null() {
                None
            } else {
                Some(
                    CStr::from_ptr(d.model)
                        .to_string_lossy()
                        .into_owned()
                        .to_string(),
                )
            });
            free(d.model as *mut c_void);

            let size = TargetSize(if d.size_is_known != 0 {
                Some(ByteSize::b(d.size))
            } else {
                None
            });

            let removable = match d.is_removable {
                0 => Removable::No,
                1 => Removable::Yes,
                _ => Removable::Unknown,
            };

            let target_type = match d.dev_type {
                native::DEV_TYPE_DISK => Type::Disk,
                native::DEV_TYPE_PARTITION => Type::Partition,
                _ => Type::File,
            };

            out.push(BurnTarget {
                name: bsdname,
                devnode,
                size,
                model,
                removable,
                target_type,
            })
        }

        free(list.disks as *mut c_void);
    }

    out.sort();

    out.into_iter()
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct BurnTarget {
    /// A user-friendly name for the disk (i.e. sda, nvme0n1, disk1s4)
    pub name: String,
    pub devnode: PathBuf,
    pub size: TargetSize,
    pub model: Model,
    pub removable: Removable,
    pub target_type: Type,
}

impl BurnTarget {
    #[cfg(target_os = "macos")]
    fn from_dev_name(name: &OsStr) -> Result<Self, DeviceParseError> {
        use std::os::unix::prelude::OsStrExt;

        use format_bytes::format_bytes;

        // I don't want to write more Objective C. Oh god. Please no.
        let devices: Vec<BurnTarget> = enumerate_devices().collect();
        let expected_if_direct_node = format_bytes!(b"/dev/{}", name.as_bytes());
        let expected_if_raw_node = format_bytes!(b"/dev/r{}", name.as_bytes());

        if let Some(found) = devices.into_iter().find(|t| {
            let bytes = t.devnode.as_os_str().as_bytes();
            bytes == &expected_if_direct_node || bytes == &expected_if_raw_node
        }) {
            Ok(found)
        } else {
            Err(DeviceParseError::NotFound)
        }
    }

    #[cfg(target_os = "linux")]
    fn from_dev_name(name: &OsStr) -> Result<Self, DeviceParseError> {
        use std::fs::read_to_string;

        fn read_sys_file(p: impl AsRef<Path>) -> Result<Option<String>, std::io::Error> {
            into_none_if_not_exists(read_to_string(p).map(|s| s.trim().to_owned()))
        }

        fn into_none_if_not_exists<T>(
            r: Result<T, std::io::Error>,
        ) -> Result<Option<T>, std::io::Error> {
            match r {
                Ok(x) => Ok(Some(x)),
                Err(e) => match e.kind() {
                    std::io::ErrorKind::NotFound => Ok(None),
                    _ => Err(e),
                },
            }
        }

        let devnode = PathBuf::from("/dev").join(name);
        if !devnode.exists() {
            return Err(DeviceParseError::NotFound);
        }

        let sysnode = PathBuf::from("/sys/class/block").join(name);

        let removable = match read_sys_file(sysnode.join("removable"))?
            .as_ref()
            .map(String::as_ref)
        {
            Some("0") => Removable::No,
            Some("1") => Removable::Yes,
            _ => Removable::Unknown,
        };

        let size = TargetSize(
            read_sys_file(sysnode.join("size"))?
                .and_then(|s| s.parse::<u64>().ok().map(|n| ByteSize::b(n * 512))),
        );

        let model =
            Model(read_sys_file(sysnode.join("device/model"))?.map(|m| m.trim().to_owned()));

        let target_type = match sysnode.join("partition").exists() {
            true => Type::Partition,
            false => Type::Disk,
        };

        Ok(Self {
            name: name.to_string_lossy().into(),
            devnode,
            size,
            removable,
            model,
            target_type,
        })
    }

    fn from_normal_file(path: PathBuf) -> Result<Self, DeviceParseError> {
        Ok(BurnTarget {
            name: path.to_string_lossy().into(),
            devnode: path,
            size: TargetSize(None),
            model: Model(None),
            removable: Removable::Unknown,
            target_type: Type::File,
        })
    }
}

impl PartialOrd for BurnTarget {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        self.devnode.partial_cmp(&other.devnode)
    }
}

impl Ord for BurnTarget {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.devnode.cmp(&other.devnode)
    }
}

impl TryFrom<&Path> for BurnTarget {
    type Error = DeviceParseError;

    fn try_from(value: &Path) -> Result<Self, Self::Error> {
        #[cfg(target_os = "linux")]
        if value.starts_with("/sys/class/block") || value.starts_with("/dev") {
            if let Some(n) = value.file_name() {
                return Ok(Self::from_dev_name(n)?);
            }
        }

        #[cfg(target_os = "macos")]
        if value.starts_with("/dev") {
            if let Some(n) = value.file_name() {
                return Ok(Self::from_dev_name(n)?);
            }
        }

        Ok(Self::from_normal_file(value.to_owned())?)
    }
}

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum DeviceParseError {
    #[error("Could not find file")]
    NotFound,
    #[error("IO error:")]
    IO(#[from] io::Error),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Model(Option<String>);

impl Display for Model {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.0 {
            Some(m) => write!(f, "{m}"),
            None => write!(f, "[unknown model]"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetSize(Option<ByteSize>);

impl Display for TargetSize {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.0 {
            Some(s) => write!(f, "{s}"),
            None => write!(f, "[unknown size]"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Removable {
    Yes,
    No,
    Unknown,
}

impl From<Option<bool>> for Removable {
    fn from(value: Option<bool>) -> Self {
        match value {
            Some(true) => Self::Yes,
            Some(false) => Self::No,
            None => Self::Unknown,
        }
    }
}

impl Display for Removable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            match self {
                Removable::Yes => "yes",
                Removable::No => "no",
                Removable::Unknown => "unknown",
            }
        )
    }
}

#[derive(Debug, PartialEq, Eq, Clone, Copy, Serialize, Deserialize, Valuable)]
pub enum Type {
    File,
    Disk,
    Partition,
}

impl Display for Type {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            match self {
                Type::File => "file",
                Type::Disk => "disk",
                Type::Partition => "partition",
            }
        )
    }
}
