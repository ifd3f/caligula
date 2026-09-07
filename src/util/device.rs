use std::{
    ffi::OsStr,
    fmt::Display,
    io,
    path::{Path, PathBuf},
};

use bytesize::ByteSize;
use serde::{Deserialize, Serialize};

#[cfg(target_os = "linux")]
pub fn enumerate_devices() -> impl Iterator<Item = WriteTarget> {
    use std::fs::read_dir;

    let paths = read_dir("/sys/class/block").unwrap();

    paths
        .filter_map(|r| r.ok())
        .filter_map(|d| WriteTarget::try_from(d.path().as_ref()).ok())
}

/// Buses whose devices can be attached and detached while the machine is
/// running.
///
/// Sitting on one of these is what makes a disk something the user can unplug,
/// and it is deliberately *not* the same question as
/// `/sys/class/block/<dev>/removable`, which reports whether the medium leaves
/// the drive. A USB stick whose flash is fixed inside its case answers "no" to
/// that and "yes" to this one. See [`is_hotplug_bus`].
///
/// `usb` is the entry with a measurement behind it; `mmc`, `memstick`,
/// `ieee1394` and `pcmcia` are here on the same reasoning, but UNVERIFIED — no
/// device on those buses was available to check.
#[cfg(target_os = "linux")]
const HOTPLUG_SUBSYSTEMS: &[&str] = &["usb", "mmc", "memstick", "ieee1394", "pcmcia"];

/// Whether the block device at `sysnode` hangs off a [hot-pluggable
/// bus](HOTPLUG_SUBSYSTEMS), found by walking its device-tree ancestors and
/// reading the subsystem each one links to.
///
/// This exists because `removable` alone is not enough to find the disks a user
/// might want to write to. Measured 2026-09-06 on a Kingston DataTraveler Max:
/// `/sys/class/block/sda/removable` read `0` while its ancestry ran
/// `.../usb2/2-3/2-3:1.0/host1/...`, so filtering on that attribute hid the
/// only disk the user could safely pick. `lsblk` agreed the drive was
/// detachable — `RM=0 HOTPLUG=1 TRAN=usb` — because it asks this question
/// instead.
///
/// The macOS side has never had this problem: `enumdisk.m` already treats a
/// disk as removable when it is *ejectable*, which is the same idea by another
/// name.
///
/// Partitions have no `device` link — measured the same day on
/// `/sys/class/block/nvme0n1p1`, which has neither that nor `removable` — so
/// they answer `false` here and stay out of the disk list.
#[cfg(target_os = "linux")]
fn is_hotplug_bus(sysnode: &Path) -> bool {
    let Ok(mut dir) = sysnode.join("device").canonicalize() else {
        return false;
    };

    loop {
        if let Ok(subsystem) = dir.join("subsystem").canonicalize()
            && let Some(name) = subsystem.file_name().and_then(OsStr::to_str)
            && HOTPLUG_SUBSYSTEMS.contains(&name)
        {
            return true;
        }

        match dir.parent() {
            Some(parent) => dir = parent.to_owned(),
            None => return false,
        }
    }
}

/// Combine the two things Linux tells us about detachability: the contents of
/// `/sys/class/block/<dev>/removable`, and whether the device sits on a
/// [hot-pluggable bus](HOTPLUG_SUBSYSTEMS).
///
/// Either one saying yes is enough. An unreadable attribute is only
/// [`Removable::Unknown`] when the bus does not settle it.
#[cfg(target_os = "linux")]
fn resolve_removable(removable_attr: Option<&str>, hotplug_bus: bool) -> Removable {
    match removable_attr {
        Some("1") => Removable::Yes,
        _ if hotplug_bus => Removable::Yes,
        Some("0") => Removable::No,
        _ => Removable::Unknown,
    }
}

#[cfg(target_os = "macos")]
pub fn enumerate_devices() -> impl Iterator<Item = WriteTarget> {
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
            let devnode = Path::new("/dev").join(rawdevname);
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

            let block_size = BlockSize(Some(ByteSize::b(d.block_size)));

            out.push(WriteTarget {
                name: bsdname,
                devnode,
                size,
                model,
                removable,
                target_type,
                block_size,
            })
        }

        free(list.disks as *mut c_void);
    }

    out.sort();

    out.into_iter()
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct WriteTarget {
    /// A user-friendly name for the disk (i.e. sda, nvme0n1, disk1s4)
    pub name: String,
    pub devnode: PathBuf,
    pub size: TargetSize,
    pub model: Model,
    pub removable: Removable,
    pub target_type: Type,
    pub block_size: BlockSize,
}

impl WriteTarget {
    #[cfg(target_os = "macos")]
    fn from_dev_name(name: &OsStr) -> Result<Self, DeviceParseError> {
        use std::os::unix::prelude::OsStrExt;

        use format_bytes::format_bytes;

        // I don't want to write more Objective C. Oh god. Please no.
        let devices: Vec<WriteTarget> = enumerate_devices().collect();
        let expected_if_direct_node = format_bytes!(b"/dev/{}", name.as_bytes());
        let expected_if_raw_node = format_bytes!(b"/dev/r{}", name.as_bytes());

        if let Some(found) = devices.into_iter().find(|t| {
            let bytes = t.devnode.as_os_str().as_bytes();
            bytes == expected_if_direct_node || bytes == expected_if_raw_node
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

        let devnode = Path::new("/dev").join(name);
        if !devnode.exists() {
            return Err(DeviceParseError::NotFound);
        }

        let sysnode = Path::new("/sys/class/block").join(name);

        let removable = resolve_removable(
            read_sys_file(sysnode.join("removable"))?.as_deref(),
            is_hotplug_bus(&sysnode),
        );

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

        let block_size = BlockSize(
            read_sys_file(sysnode.join("queue/physical_block_size"))?
                .and_then(|s| s.parse::<u64>().ok())
                .map(ByteSize::b),
        );

        Ok(Self {
            name: name.to_string_lossy().into(),
            devnode,
            size,
            removable,
            model,
            target_type,
            block_size,
        })
    }

    fn from_normal_file(path: &Path) -> Result<Self, DeviceParseError> {
        Ok(WriteTarget {
            name: path.to_string_lossy().into(),
            devnode: path.into(),
            size: TargetSize(None),
            model: Model(None),
            removable: Removable::Unknown,
            target_type: Type::File,
            block_size: BlockSize(None),
        })
    }
}

impl PartialOrd for WriteTarget {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for WriteTarget {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.devnode.cmp(&other.devnode)
    }
}

impl TryFrom<&Path> for WriteTarget {
    type Error = DeviceParseError;

    fn try_from(value: &Path) -> Result<Self, Self::Error> {
        #[cfg(target_os = "linux")]
        if (value.starts_with("/sys/class/block") || value.starts_with("/dev"))
            && let Some(n) = value.file_name()
        {
            return Self::from_dev_name(n);
        }

        #[cfg(target_os = "macos")]
        if value.starts_with("/dev")
            && let Some(n) = value.file_name()
        {
            return Self::from_dev_name(n);
        }

        Self::from_normal_file(value)
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

#[derive(Debug, Clone, PartialEq, Eq, derive_more::From)]
pub struct Model(Option<String>);

impl Display for Model {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.0 {
            Some(m) => write!(f, "{m}"),
            None => write!(f, "[unknown model]"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, derive_more::From)]
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

#[derive(Debug, PartialEq, Eq, Clone, Copy, Serialize, Deserialize)]
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

#[derive(Debug, Clone, PartialEq, Eq, derive_more::From)]
pub struct BlockSize(pub Option<ByteSize>);

impl Display for BlockSize {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.0 {
            Some(bs) => write!(f, "{}", bs),
            None => write!(f, "[unknown block size]"),
        }
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use std::{env, fs, os::unix::fs::symlink, path::PathBuf, process};

    use test_case::test_case;

    use super::{Removable, is_hotplug_bus, resolve_removable};

    /// Builds a throwaway sysfs-shaped tree and returns the path standing in
    /// for `/sys/class/block/<dev>`.
    ///
    /// `device_chain` runs from the top of the device tree down to the node
    /// that `<dev>/device` points at, naming the subsystem each level links to
    /// where it has one.
    fn fake_sysnode(case: &str, device_chain: &[(&str, &str)]) -> PathBuf {
        let root = env::temp_dir().join(format!("caligula-test-{case}-{}", process::id()));
        let _ = fs::remove_dir_all(&root);

        let mut device = root.join("devices");
        fs::create_dir_all(&device).unwrap();

        for (name, subsystem) in device_chain {
            device = device.join(name);
            fs::create_dir_all(&device).unwrap();

            let bus = root.join("bus").join(subsystem);
            fs::create_dir_all(&bus).unwrap();
            symlink(&bus, device.join("subsystem")).unwrap();
        }

        let sysnode = root.join("class").join("block").join("disk");
        fs::create_dir_all(&sysnode).unwrap();
        symlink(&device, sysnode.join("device")).unwrap();

        sysnode
    }

    #[test]
    fn usb_flash_drive_is_on_a_hotplug_bus() {
        // The ancestry of a Kingston DataTraveler Max, read off
        // /sys/class/block/sda on 2026-09-06: a SCSI chain hanging off a USB
        // host controller.
        let sysnode = fake_sysnode(
            "usb",
            &[
                ("pci0000:00", "pci"),
                ("usb2", "usb"),
                ("2-3", "usb"),
                ("2-3:1.0", "usb"),
                ("host1", "scsi"),
                ("target1:0:0", "scsi"),
                ("1:0:0:0", "scsi"),
            ],
        );

        assert!(is_hotplug_bus(&sysnode));
    }

    #[test]
    fn internal_nvme_is_not_on_a_hotplug_bus() {
        // Negative control: the ancestry of /sys/class/block/nvme0n1 on the
        // same machine, same day.
        let sysnode = fake_sysnode(
            "nvme",
            &[
                ("pci0000:00", "pci"),
                ("0000:02:00.0", "pci"),
                ("nvme0", "nvme"),
            ],
        );

        assert!(!is_hotplug_bus(&sysnode));
    }

    #[test]
    fn node_without_a_device_link_is_not_on_a_hotplug_bus() {
        // Partitions are the real instance of this: measured 2026-09-06,
        // /sys/class/block/nvme0n1p1 has neither `removable` nor `device`.
        let sysnode = env::temp_dir().join(format!("caligula-test-bare-{}", process::id()));
        let _ = fs::remove_dir_all(&sysnode);
        fs::create_dir_all(&sysnode).unwrap();

        assert!(!is_hotplug_bus(&sysnode));
    }

    #[test_case(Some("1"), false => Removable::Yes; "removable medium")]
    #[test_case(Some("1"), true => Removable::Yes; "removable medium on hotplug bus")]
    #[test_case(Some("0"), true => Removable::Yes; "fixed medium on hotplug bus")]
    #[test_case(None, true => Removable::Yes; "unreadable attribute on hotplug bus")]
    #[test_case(Some("0"), false => Removable::No; "fixed medium on fixed bus")]
    #[test_case(None, false => Removable::Unknown; "nothing known")]
    fn resolves_removable(removable_attr: Option<&str>, hotplug_bus: bool) -> Removable {
        resolve_removable(removable_attr, hotplug_bus)
    }
}
