//! Linux mount discovery and unmounting before opening a write target.

use std::{
    ffi::OsString,
    fs, io,
    os::{
        fd::AsRawFd,
        unix::{
            ffi::OsStringExt,
            fs::{MetadataExt, OpenOptionsExt},
        },
    },
    path::{Path, PathBuf},
};

use nix::mount::{MntFlags, umount2};

use super::Mount;

pub fn mountpoints(target: &Path) -> io::Result<Vec<Mount>> {
    let dev = fs::metadata(target)?.rdev();
    let sysnode = PathBuf::from(format!(
        "/sys/dev/block/{}:{}",
        libc::major(dev),
        libc::minor(dev)
    ));
    let mut devices = vec![dev];
    for entry in fs::read_dir(sysnode)? {
        let entry = entry?;
        let path = entry.path();
        if entry.file_type()?.is_dir() && path.join("partition").try_exists()? {
            devices.push(parse_device(
                fs::read_to_string(path.join("dev"))?.trim().as_bytes(),
            )?);
        }
    }

    parse_mountinfo(&fs::read("/proc/self/mountinfo")?, &devices)
}

fn parse_device(field: &[u8]) -> io::Result<libc::dev_t> {
    let invalid = || io::Error::new(io::ErrorKind::InvalidData, "Invalid mount device number");
    let (major, minor) = std::str::from_utf8(field)
        .ok()
        .and_then(|s| s.split_once(':'))
        .ok_or_else(invalid)?;
    Ok(libc::makedev(
        major.parse().map_err(|_| invalid())?,
        minor.parse().map_err(|_| invalid())?,
    ))
}

fn parse_mountinfo(input: &[u8], devices: &[libc::dev_t]) -> io::Result<Vec<Mount>> {
    let mut mounts = Vec::new();
    for line in input.split(|b| *b == b'\n').filter(|l| !l.is_empty()) {
        let fields: Vec<_> = line.split(|b| *b == b' ').collect();
        let separator = fields.iter().position(|f| *f == b"-");
        let Some(separator) = separator.filter(|s| *s >= 6 && fields.len() >= s + 4) else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Invalid mountinfo entry",
            ));
        };
        let dev = parse_device(fields[2])?;
        // Btrfs reports an anonymous device number in mountinfo. Its source
        // identifies the block device for the usual single-device filesystem.
        let btrfs_on_target = if fields[separator + 1] == b"btrfs" {
            fs::metadata(unescape_path(fields[separator + 2])?)
                .is_ok_and(|m| devices.contains(&m.rdev()))
        } else {
            false
        };
        if devices.contains(&dev) || btrfs_on_target {
            let id = std::str::from_utf8(fields[0])
                .ok()
                .and_then(|s| s.parse().ok())
                .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "Invalid mount ID"))?;
            mounts.push(Mount {
                id,
                path: unescape_path(fields[4])?,
            });
        }
    }
    // Children and bind mounts must be detached before their parents.
    mounts.sort_by_key(|m| std::cmp::Reverse(m.path.components().count()));
    Ok(mounts)
}

fn unescape_path(field: &[u8]) -> io::Result<PathBuf> {
    let mut bytes = Vec::with_capacity(field.len());
    let mut remaining = field;
    while let Some((&byte, rest)) = remaining.split_first() {
        if byte == b'\\' {
            let escaped = match remaining.get(..4) {
                Some(b"\\040") => b' ',
                Some(b"\\011") => b'\t',
                Some(b"\\012") => b'\n',
                Some(b"\\134") => b'\\',
                _ => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "Invalid mount path escape",
                    ));
                }
            };
            bytes.push(escaped);
            remaining = &remaining[4..];
        } else {
            bytes.push(byte);
            remaining = rest;
        }
    }
    Ok(OsString::from_vec(bytes).into())
}

fn visible_mount_id(path: &Path) -> io::Result<u64> {
    let file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_PATH | libc::O_NOFOLLOW)
        .open(path)?;
    let info = fs::read_to_string(format!("/proc/self/fdinfo/{}", file.as_raw_fd()))?;
    info.lines()
        .find_map(|line| line.strip_prefix("mnt_id:"))
        .and_then(|id| id.trim().parse().ok())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "Missing mount ID in fdinfo"))
}

fn ensure_visible(mount: &Mount) -> io::Result<()> {
    if visible_mount_id(&mount.path)? != mount.id {
        return Err(io::Error::other(format!(
            "{} is covered by another mount or has been replaced. Refusing to unmount it.",
            mount.path.display()
        )));
    }
    Ok(())
}

pub fn unmount(target: &Path, approved: &[Mount]) -> io::Result<()> {
    unmount_all(&mountpoints(target)?, approved, |mount| {
        // Never force or lazily detach: a busy filesystem must prevent writing.
        umount2(&mount.path, MntFlags::UMOUNT_NOFOLLOW).map_err(|e| {
            io::Error::new(
                io::Error::from_raw_os_error(e as i32).kind(),
                format!("Could not unmount {}: {e}", mount.path.display()),
            )
        })
    })
}

fn unmount_all(
    current: &[Mount],
    approved: &[Mount],
    mut unmount_one: impl FnMut(&Mount) -> io::Result<()>,
) -> io::Result<()> {
    for mount in current {
        if !approved.contains(mount) {
            return Err(io::Error::other(format!(
                "{} was mounted after confirmation. Select the target again to approve unmounting \
                 it.",
                mount.path.display()
            )));
        }
    }
    // Check all mountpoints before detaching any of them.
    for mount in current {
        ensure_visible(mount)?;
    }
    for mount in current {
        // Recheck immediately before the path-based syscall.
        ensure_visible(mount)?;
        unmount_one(mount)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_whole_disk_partitions_and_bind_mounts_in_unmount_order() {
        let info = b"20 1 8:0 / /disk rw - ext4 /dev/sda rw\n\
            21 1 8:1 / /media/USB\\040drive rw shared:1 - vfat /dev/disk/by-uuid/123 rw\n\
            22 21 8:1 /folder /media/USB\\040drive/bind rw - vfat /dev/sda1 rw\n\
            23 1 8:16 / /unrelated rw - ext4 /dev/sdb rw\n\
            24 1 0:42 / /tmp rw - tmpfs tmpfs rw\n";
        assert_eq!(
            parse_mountinfo(info, &[libc::makedev(8, 0), libc::makedev(8, 1)]).unwrap(),
            vec![
                Mount {
                    id: 22,
                    path: "/media/USB drive/bind".into()
                },
                Mount {
                    id: 21,
                    path: "/media/USB drive".into()
                },
                Mount {
                    id: 20,
                    path: "/disk".into()
                },
            ]
        );
        // Selecting one partition must not include the whole disk or siblings.
        assert_eq!(
            parse_mountinfo(info, &[libc::makedev(8, 1)]).unwrap().len(),
            2
        );
        assert!(
            parse_mountinfo(info, &[libc::makedev(8, 2)])
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn decodes_mount_paths_without_requiring_utf8() {
        let path = unescape_path(b"/media/\\134040\\011\\012\xff").unwrap();
        assert_eq!(path.into_os_string().into_vec(), b"/media/\\040\t\n\xff");
        assert!(unescape_path(b"/bad\\04").is_err());
    }

    #[test]
    fn malformed_mountinfo_is_an_error() {
        assert!(parse_mountinfo(b"not a mount entry", &[]).is_err());
        assert!(parse_mountinfo(b"1 2 invalid / / rw - ext4 /dev/sda rw", &[]).is_err());
        assert!(
            parse_mountinfo(
                b"bad 2 8:0 / / rw - ext4 /dev/sda rw",
                &[libc::makedev(8, 0)]
            )
            .is_err()
        );
    }

    #[test]
    fn new_mounts_are_rejected_before_any_unmount() {
        let mounts = vec![
            Mount {
                id: 1,
                path: "/approved".into(),
            },
            Mount {
                id: 2,
                path: "/new".into(),
            },
        ];
        assert!(unmount_all(&mounts, &mounts[..1], |_| panic!("must not unmount")).is_err());
    }

    #[test]
    fn replacement_at_approved_path_is_not_approved() {
        let approved = Mount {
            id: 1,
            path: "/target".into(),
        };
        let replacement = Mount {
            id: 2,
            ..approved.clone()
        };
        assert!(unmount_all(&[replacement], &[approved], |_| panic!("must not unmount")).is_err());
    }

    fn root_mount() -> Mount {
        Mount {
            id: visible_mount_id(Path::new("/")).unwrap(),
            path: "/".into(),
        }
    }

    #[test]
    fn covered_mount_is_rejected_before_any_unmount() {
        let visible = root_mount();
        let covered = Mount {
            id: 0,
            ..visible.clone()
        };
        let mounts = [visible, covered];
        let error = unmount_all(&mounts, &mounts, |_| panic!("must not unmount")).unwrap_err();
        assert!(error.to_string().contains("covered by another mount"));
    }

    #[test]
    fn unmounts_current_mounts_only_and_stops_on_failure() {
        let mounts = vec![root_mount(), root_mount()];
        let mut calls = Vec::new();
        let error = unmount_all(&mounts, &mounts, |p| {
            calls.push(p.clone());
            Err(io::Error::from_raw_os_error(libc::EBUSY))
        })
        .unwrap_err();
        assert_eq!(error.raw_os_error(), Some(libc::EBUSY));
        assert_eq!(calls, mounts[..1]);
        unmount_all(&[], &mounts, |_| panic!("already unmounted")).unwrap();
        unmount_all(&mounts, &mounts, |_| Ok(())).unwrap();
    }
}
