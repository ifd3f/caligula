use std::{fmt, io};

use inquire::{Confirm, InquireError, Select};
use tracing::debug;

use crate::{
    codec::compression::{AVAILABLE_FORMATS, CompressionArg, CompressionFormat},
    facade::WriteVerifyWorkflow,
    tui::cli::BurnArgs,
    util::device::{self, Mount, Removable, WriteTarget, enumerate_devices},
};

#[tracing::instrument(skip_all)]
pub fn ask_compression(args: &BurnArgs) -> anyhow::Result<CompressionFormat> {
    let cf = match args.compression {
        CompressionArg::Auto | CompressionArg::Ask => {
            CompressionFormat::detect_from_path(&args.image)
        }
        other => other.associated_format(),
    };

    if let Some(cf) = cf {
        eprintln!("Input file: {}", args.image.to_string_lossy());
        eprintln!("Detected compression format: {}", cf);

        if args.force || args.compression != CompressionArg::Ask {
            return Ok(cf);
        }

        if !Confirm::new("Is this okay?").with_default(true).prompt()? {
            Err(InquireError::OperationCanceled)?;
        }
        return Ok(cf);
    }

    eprintln!(
        "Couldn't detect compression format for {}",
        args.image.to_string_lossy()
    );
    if args.force {
        eprintln!("Since --force was provided, assuming it's uncompressed!");
        return Ok(CompressionFormat::Identity);
    }
    let format = Select::new("What format to use?", AVAILABLE_FORMATS.to_vec()).prompt()?;

    Ok(format)
}

#[tracing::instrument(skip_all)]
pub fn ask_outfile(args: &BurnArgs) -> anyhow::Result<WriteTarget> {
    let mut show_all_disks = args.show_all_disks;

    loop {
        debug!(show_all_disks, "Beginning loop");

        let targets = enumerate_options(show_all_disks, enumerate_devices())?;

        let ans = Select::new("Select target disk", targets)
            .with_help_message(if show_all_disks {
                "Showing all disks. Proceed with caution!"
            } else {
                "Only displaying removable disks."
            })
            .prompt()?;

        let dev = match ans {
            ListOption::Device(dev, _) => dev,
            ListOption::RetryWithShowAll(sa) => {
                show_all_disks = sa;
                continue;
            }
            ListOption::Refresh => {
                continue;
            }
        };
        return Ok(dev);
    }
}

pub fn confirm_unmount(
    args: &BurnArgs,
    target: &WriteTarget,
) -> anyhow::Result<Option<Vec<Mount>>> {
    let mounts = target.mountpoints()?;
    if mounts.is_empty() {
        return Ok(Some(mounts));
    }

    eprintln!("Mounted filesystems on {}:", target.devnode.display());
    for mount in &mounts {
        eprintln!("  {}", mount.path.display());
    }
    if args.force {
        eprintln!("Since --force was provided, these will be unmounted before writing.");
        return Ok(Some(mounts));
    }
    if Confirm::new("Unmount these filesystems before writing?")
        .with_default(false)
        .with_help_message(
            "Choose no to return to disk selection. Close any files on the drive first.",
        )
        .prompt()?
    {
        Ok(Some(mounts))
    } else {
        Ok(None)
    }
}

#[tracing::instrument(skip_all)]
pub fn confirm_write(
    args: &BurnArgs,
    begin_params: &WriteVerifyWorkflow,
) -> Result<bool, InquireError> {
    if args.force {
        debug!("Skipping confirm because of --force");
        Ok(true)
    } else {
        print_begin_params(begin_params);

        Confirm::new("Is this okay?")
            .with_help_message("THIS ACTION WILL DESTROY ALL DATA ON THIS DEVICE!!!")
            .with_default(false)
            .prompt()
    }
}

fn print_begin_params(params: &WriteVerifyWorkflow) {
    println!("Input: {}", params.input_file.to_string_lossy());
    if params.compression.is_identity() {
        println!("  Size: {}", params.input_file_size);
    } else {
        println!("  Size (compressed): {}", params.input_file_size);
    }
    println!("  Compression: {}", params.compression);
    println!();

    println!("Output: {}", params.target.name);
    println!("  Model: {}", params.target.model);
    println!("  Size: {}", params.target.size);
    println!("  Block size: {}", params.target.block_size);
    println!("  Type: {}", params.target.target_type);
    println!("  Path: {}", params.target.devnode.to_string_lossy());

    if params.target.target_type == device::Type::Disk {
        println!("  Removable: {}", params.target.removable);
    }
}

enum ListOption {
    Device(WriteTarget, bool),
    Refresh,
    RetryWithShowAll(bool),
}

impl fmt::Display for ListOption {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ListOption::Device(dev, mounted) => {
                match dev.target_type {
                    device::Type::Disk => write!(
                        f,
                        "{} | {} - {} ({}, removable: {})",
                        dev.name, dev.model, dev.size, dev.target_type, dev.removable
                    )?,
                    _ => write!(
                        f,
                        "{} | {} - {} ({})",
                        dev.name, dev.model, dev.size, dev.target_type
                    )?,
                }
                if *mounted {
                    write!(f, " [mounted]")?;
                }
            }
            ListOption::RetryWithShowAll(true) => {
                write!(f, "<Show all disks, removable or not>")?;
            }
            ListOption::RetryWithShowAll(false) => {
                write!(f, "<Only show removable disks>")?;
            }
            ListOption::Refresh => {
                write!(f, "<Refresh devices>")?;
            }
        }
        Ok(())
    }
}

#[tracing::instrument(skip(devices))]
fn enumerate_options(
    show_all_disks: bool,
    devices: impl Iterator<Item = WriteTarget>,
) -> anyhow::Result<Vec<ListOption>> {
    let mut burn_targets: Vec<WriteTarget> = devices
        .filter(|d| show_all_disks || d.removable == Removable::Yes)
        .collect();

    burn_targets.sort();

    let devices = burn_targets
        .into_iter()
        .filter_map(|dev| match dev.mountpoints() {
            Ok(mounts) => Some(Ok(ListOption::Device(dev, !mounts.is_empty()))),
            // Hot-unplug may remove a device after enumeration.
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Err(error) => Some(Err(error)),
        })
        .collect::<std::io::Result<Vec<_>>>()?;
    let options = devices.into_iter().chain([
        ListOption::Refresh,
        ListOption::RetryWithShowAll(!show_all_disks),
    ]);

    Ok(options.collect())
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use std::path::Path;

    use super::*;

    #[test]
    fn vanished_device_does_not_hide_healthy_targets() {
        let mut vanished =
            WriteTarget::try_from(Path::new("/proc/self/caligula-missing-device")).unwrap();
        vanished.target_type = device::Type::Disk;
        // The same error must still stop a selected target before writing.
        assert_eq!(
            vanished.mountpoints().unwrap_err().kind(),
            io::ErrorKind::NotFound
        );
        let healthy = WriteTarget::try_from(Path::new("/tmp/healthy-target")).unwrap();
        let options = enumerate_options(true, [vanished, healthy.clone()].into_iter()).unwrap();
        assert_eq!(options.len(), 3);
        assert!(matches!(&options[0], ListOption::Device(dev, false) if *dev == healthy));
        assert!(matches!(options[1], ListOption::Refresh));
        assert!(matches!(options[2], ListOption::RetryWithShowAll(false)));
    }
}
