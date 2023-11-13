use std::{fmt, time::Instant};

use indicatif::{ProgressBar, ProgressStyle};
use inquire::{Confirm, InquireError, Select};
use tracing::debug;

use crate::{
    device::{enumerate_devices, Removable, WriteTarget},
    ui::{burn::start::try_start_burn, cli::BurnArgs},
    writer_process::state_tracking::WriterState,
};

use super::start::InputFileParams;

pub struct InputAndTarget {
    input: InputFileParams,
    target: WriteTarget,
}

impl fmt::Display for InputAndTarget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "self.input: {}", self.input.file.to_string_lossy())?;
        if self.input.compression.is_identity() {
            writeln!(f, "  Size: {}", self.input.size)?;
        } else {
            writeln!(f, "  Size (compressed): {}", self.input.size)?;
        }
        writeln!(f, "  Compression: {}", self.input.compression)?;
        writeln!(f)?;

        writeln!(f, "Output: {}", self.target.name)?;
        writeln!(f, "  Model: {}", self.target.model)?;
        writeln!(f, "  Size: {}", self.target.size)?;
        writeln!(f, "  Type: {}", self.target.target_type)?;
        writeln!(f, "  Path: {}", self.target.devnode.to_string_lossy())?;
        writeln!(f, "  Removable: {}", self.target.removable)?;

        Ok(())
    }
}

pub async fn run_simple_ui(args: &BurnArgs, input: InputFileParams) -> anyhow::Result<()> {
    let target = match &args.out {
        Some(f) => WriteTarget::try_from(f.as_ref())?,
        None => ask_outfile(args)?,
    };

    let params = InputAndTarget { input, target };

    confirm_write(args, &params);

    do_burn(&args, &params);
    Ok(())
}

async fn do_burn(args: &BurnArgs, params: &InputAndTarget) -> anyhow::Result<()> {
    let writer_config = params.input.make_writer_config(&params.target);
    let mut handle =
        try_start_burn(&writer_config, args.root, args.interactive.is_interactive()).await?;

    let input_file_bytes = handle.initial_info().input_file_bytes;
    let write_progress = ProgressBar::new(100).with_message("Burning").with_style(
        ProgressStyle::with_template(
            "[{elapsed_precise}] {msg:>10} {wide_bar:.green/black} {percent:>3}%",
        )
        .unwrap(),
    );
    let verify_progress = ProgressBar::new(100).with_message("Verifying").with_style(
        ProgressStyle::with_template(
            "[{elapsed_precise}] {msg:>10} {wide_bar:.blue/black} {percent:>3}%",
        )
        .unwrap(),
    );

    let mut child_state = WriterState::initial(
        Instant::now(),
        !params.input.compression.is_identity(),
        input_file_bytes,
    );

    loop {
        let x = handle.next_message().await?;
        child_state = child_state.on_status(Instant::now(), x);
        match &child_state {
            WriterState::Writing(b) => {
                write_progress.set_position((b.approximate_ratio() * 1000.0) as u64)
            }
            WriterState::Verifying {
                total_write_bytes, ..
            } => verify_progress.set_position(total_write_bytes * 1000 / input_file_bytes),
            WriterState::Finished { .. } => break,
        }
    }
    println!("Done!");
    Ok(())
}

#[tracing::instrument(skip_all)]
pub fn ask_outfile(args: &BurnArgs) -> anyhow::Result<WriteTarget> {
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

#[tracing::instrument(skip_all)]
pub fn confirm_write(args: &BurnArgs, begin_params: &InputAndTarget) -> Result<bool, InquireError> {
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
    Device(WriteTarget),
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

#[tracing::instrument]
fn enumerate_options(show_all_disks: bool) -> anyhow::Result<Vec<ListOption>> {
    let mut burn_targets: Vec<WriteTarget> = enumerate_devices()
        .filter(|d| show_all_disks || d.removable == Removable::Yes)
        .collect();

    burn_targets.sort();

    let options = burn_targets.into_iter().map(ListOption::Device).chain([
        ListOption::Refresh,
        ListOption::RetryWithShowAll(!show_all_disks),
    ]);

    Ok(options.collect())
}
