use std::{
    ffi::OsStr,
    fmt::Display,
    fs::read_to_string,
    io,
    path::{Path, PathBuf},
};

use bytesize::ByteSize;

#[cfg(target_os = "linux")]
pub fn enumerate_devices() -> impl Iterator<Item = BurnTarget> {
    use std::fs::read_dir;

    let paths = read_dir("/sys/class/block").unwrap();

    paths
        .filter_map(|r| r.ok())
        .filter_map(|d| BurnTarget::try_from(d.path().as_ref()).ok())
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct BurnTarget {
    pub devnode: PathBuf,
    pub size: TargetSize,
    pub model: Model,
    pub removable: Removable,
    pub target_type: Type,
}

impl BurnTarget {
    fn from_dev_name(name: &OsStr) -> Result<Self, DeviceParseError> {
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

        Ok(Self {
            devnode,
            size,
            removable,
            model,
            target_type: Type::Block,
        })
    }

    fn from_normal_file(path: PathBuf) -> Result<Self, DeviceParseError> {
        Ok(BurnTarget {
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

impl TryFrom<&Path> for BurnTarget {
    type Error = DeviceParseError;

    fn try_from(value: &Path) -> Result<Self, Self::Error> {
        if value.starts_with("/sys/class/block") || value.starts_with("/dev") {
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

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum Type {
    File,
    Block,
}

impl Display for Type {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            match self {
                Type::File => "file",
                Type::Block => "block",
            }
        )
    }
}

fn read_sys_file(p: impl AsRef<Path>) -> Result<Option<String>, std::io::Error> {
    into_none_if_not_exists(read_to_string(p).map(|s| s.trim().to_owned()))
}

fn into_none_if_not_exists<T>(r: Result<T, std::io::Error>) -> Result<Option<T>, std::io::Error> {
    match r {
        Ok(x) => Ok(Some(x)),
        Err(e) => match e.kind() {
            std::io::ErrorKind::NotFound => Ok(None),
            _ => Err(e),
        },
    }
}
