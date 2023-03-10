use std::fmt;

use inquire::{Confirm, InquireError, Select};
use tracing::debug;

use crate::{
    cli::BurnArgs,
    compression::{CompressionFormat, DecompressError, AVAILABLE_FORMATS},
    device::{enumerate_devices, BurnTarget, Removable},
};

use super::burn::start::BeginParams;

pub fn ask_compression(args: &BurnArgs) -> anyhow::Result<CompressionFormat> {
    if let Some(cf) = args.compression.detect_format(&args.input) {
        if args.force {
            return Ok(cf);
        }
        eprintln!("Input file: {}", args.input.to_string_lossy());
        eprintln!("Detected compression format: {}", cf);
        if !cf.is_available() {
            eprintln!(
                "Compression format {} is not supported on your platform!",
                cf
            );
            Err(DecompressError::UnsupportedFormat(cf))?;
        }
        if !Confirm::new("Is this okay?").prompt()? {
            Err(InquireError::OperationCanceled)?;
        }
        return Ok(cf);
    }

    eprintln!(
        "Couldn't detect compression format for {}",
        args.input.to_string_lossy()
    );
    if args.force {
        eprintln!("Since --force was provided, assuming it's uncompressed!");
        return Ok(CompressionFormat::Identity);
    }
    let format = Select::new("What format to use?", AVAILABLE_FORMATS.to_vec()).prompt()?;

    return Ok(format);
}

pub fn ask_outfile(args: &BurnArgs) -> anyhow::Result<BurnTarget> {
    let mut show_all_disks = args.show_all_disks;

    loop {
        debug!(show_all_disks, "Beginning loop");

        let targets = enumerate_options(show_all_disks)?;

        let ans = Select::new("Select target disk", targets)
            .with_help_message(if show_all_disks {
                "Showing all disks. Proceed with caution!"
            } else {
                "Only displaying removable disks."
            })
            .prompt()?;

        let dev = match ans {
            ListOption::Device(dev) => dev,
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

pub fn confirm_write(args: &BurnArgs, begin_params: &BeginParams) -> Result<bool, InquireError> {
    if args.force {
        debug!("Skipping confirm because of --force");
        Ok(true)
    } else {
        println!("{}", begin_params);

        Confirm::new("Is this okay?")
            .with_help_message("THIS ACTION WILL DESTROY ALL DATA ON THIS DEVICE!!!")
            .with_default(false)
            .prompt()
    }
}

enum ListOption {
    Device(BurnTarget),
    Refresh,
    RetryWithShowAll(bool),
}

impl fmt::Display for ListOption {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ListOption::Device(dev) => {
                write!(
                    f,
                    "{} | {} - {} ({}, removable: {})",
                    dev.name, dev.model, dev.size, dev.target_type, dev.removable
                )?;
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

fn enumerate_options(show_all_disks: bool) -> anyhow::Result<Vec<ListOption>> {
    let mut burn_targets: Vec<BurnTarget> = enumerate_devices()
        .filter(|d| show_all_disks || d.removable == Removable::Yes)
        .collect();

    burn_targets.sort();

    let options = burn_targets.into_iter().map(ListOption::Device).chain([
        ListOption::Refresh,
        ListOption::RetryWithShowAll(!show_all_disks),
    ]);

    Ok(options.collect())
}
