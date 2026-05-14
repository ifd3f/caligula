//! The "simple UI". This is module holds subroutines that don't use ratatui,
//! and don't take up the entire terminal screen.
//!
//! As pretty as ratatui looks, sometimes you can't use a full-featured
//! terminal. This is what this module is for.

use std::{sync::Arc, time::Duration};

use indicatif::{ProgressBar, ProgressStyle};
use inquire::Confirm;
use tracing::debug;

use self::{
    ask_hash::ask_hash,
    ask_outfile::{ask_compression, ask_outfile, confirm_write},
    facade_ext::FacadeExt as _,
};
use super::cli::BurnArgs;
use crate::{
    device::WriteTarget,
    facade::{
        CaligulaFacade, DaemonError, Orchestrator, WVState, WriteVerifyWorkflow,
        watch::Watch,
        workflow::{hash::HashWorkflow, write_verify::WriteVerifyWorkflowError},
    },
    herder_api::{error::DiskError, write_verify::LegacyWriteVerifyError},
    logging::LogPaths,
    runtime::RemoteSpawn,
    ui::cli::UseSudo,
};

mod ask_hash;
mod ask_outfile;
mod facade_ext;

/// How often we refresh the display
const REFRESH_PERIOD: Duration = Duration::from_millis(100);

/// Run the simple UI setup wizard, a cruel being that interrogates the user
/// until it is satisfied with its answers.
///
/// Returns the [BeginParams] if the user confirms, and None if the user
/// doesn't.
#[tracing::instrument(skip_all)]
pub fn do_setup_wizard(
    orc: &impl Orchestrator<HashWorkflow>,
    args: &BurnArgs,
) -> Result<Option<WriteVerifyWorkflow>, anyhow::Error> {
    let compression = ask_compression(args)?;
    let _hash_info = ask_hash(orc, args, compression)?;
    let target = match &args.out {
        Some(f) => WriteTarget::try_from(f.as_ref())?,
        None => ask_outfile(args)?,
    };
    let begin_params = WriteVerifyWorkflow::new(args.image.clone(), compression, target)?;
    if !confirm_write(args, &begin_params)? {
        eprintln!("Aborting.");
        return Ok(None);
    }
    Ok(Some(begin_params))
}

pub struct Params<'a> {
    pub child_state: Watch<WVState>,
    pub log_paths: &'a LogPaths,
}

#[derive(Debug, thiserror::Error)]
pub enum WriteOrEscalateError {
    #[error("Error spawning writer: {0}")]
    Write(#[from] Arc<WriteVerifyWorkflowError>),
    #[error("Error escalating: {0}")]
    Escalate(#[from] DaemonError),
    #[error("Not allowed to escalate")]
    NotAllowedToEscalate,
}

/// Attempt to start burning the disk with the given params.
///
/// If received permission denied, figures out if it needs to ask the user to
/// sudo based on what's provided in the `root` argument.
#[tracing::instrument(skip_all, fields(root, interactive))]
pub fn try_start_write_or_escalate(
    facade: Arc<impl CaligulaFacade>,
    runtime: &impl RemoteSpawn,
    args: &WriteVerifyWorkflow,
    root: UseSudo,
    interactive: bool,
) -> Result<Watch<WVState>, WriteOrEscalateError> {
    tracing::info!("Starting burn without escalation");

    let err = match facade
        .clone()
        .start_write_verify_blocking(runtime, args.clone())
    {
        Ok(p) => return Ok(p),
        Err(e) => e,
    };

    tracing::info!("Unescalated burn failed with error, attempting to recover: {err}");

    match err.as_ref() {
        WriteVerifyWorkflowError::Worker(e) => match e {
            LegacyWriteVerifyError::OutputFile(e)
                if e.kind() == Some(&DiskError::PermissionDenied) =>
            {
                request_escalation(runtime, facade.clone(), root, interactive, args)?;
                Ok(facade.start_write_verify_blocking(runtime, args.clone())?)
            }
            _ => Err(err.into()),
        },
        error => panic!("An unrecoverable developer error occurred: {error}"),
    }
}

fn request_escalation(
    runtime: &impl RemoteSpawn,
    facade: Arc<impl CaligulaFacade>,
    root: UseSudo,
    interactive: bool,
    args: &WriteVerifyWorkflow,
) -> Result<(), WriteOrEscalateError> {
    match (root, interactive) {
        (UseSudo::Ask, true) => {
            debug!("Failure due to insufficient perms, asking user to escalate");

            let response = Confirm::new(&format!(
                "We don't have permissions on {}. Escalate using sudo?",
                args.target.name
            ))
            .with_default(true)
            .with_help_message("We will use the sudo command, which may prompt you for a password.")
            .prompt()
            .expect("prompting the user should not fail");

            if !response {
                return Err(WriteOrEscalateError::NotAllowedToEscalate);
            }
        }
        (UseSudo::Always, _) => (),
        _ => return Err(WriteOrEscalateError::NotAllowedToEscalate),
    }

    facade.escalate_blocking(runtime, None)?;
    Ok(())
}

/// Run the simple TUI.
#[tracing::instrument(skip_all)]
pub fn run<'a>(params: Params<'a>) {
    let length = 80;
    let write_progress = ProgressBar::new(length).with_message("Burning").with_style(
        ProgressStyle::with_template(
            "[{elapsed_precise}] {msg:>10} {wide_bar:.yellow/black} {percent:>3}%",
        )
        .unwrap(),
    );
    let verify_progress = ProgressBar::new(length)
        .with_message("Verifying")
        .with_style(
            ProgressStyle::with_template(
                "[{elapsed_precise}] {msg:>10} {wide_bar:.blue/yellow} {percent:>3}%",
            )
            .unwrap(),
        );

    loop {
        std::thread::sleep(REFRESH_PERIOD);

        let child_state = params.child_state.borrow();
        match &*child_state {
            WVState::Writing(b) => {
                write_progress.set_position((b.approximate_ratio() * (length as f64)) as u64)
            }
            WVState::Verifying {
                verify_hist,
                total_write_bytes,
                ..
            } => {
                let ratio = verify_hist.bytes_encountered() as f64 / *total_write_bytes as f64;
                verify_progress.set_position((ratio * (length as f64)) as u64)
            }
            WVState::Finished { result, .. } => {
                match result {
                    Err(error) => {
                        println!("Error occurred while writing: {error}");
                        println!("{}", params.log_paths.get_bug_report_msg());
                    }
                    Ok(()) => {
                        println!("Done!")
                    }
                }
                break;
            }
        }
    }
}
