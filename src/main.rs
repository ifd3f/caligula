use crate::{
    logging::{get_log_paths, init_logging_parent},
    ui::ask_outfile,
};
use ask_outfile::ask_compression;
use burn::{
    child::is_in_burn_mode,
    handle::StartProcessError,
    ipc::{BurnConfig, ErrorType},
};
use clap::Parser;
use cli::{Args, BurnArgs, Command};
use compression::CompressionFormat;
use device::BurnTarget;
use inquire::{Confirm, InquireError};
use tracing::debug;
use ui::{confirm_write, utils::TUICapture};

pub mod burn;
pub mod cli;
mod compression;
mod device;
pub mod logging;
pub mod native;
mod ui;

fn main() {
    if is_in_burn_mode() {
        burn::child::main();
    } else {
        init_logging_parent();

        debug!("Starting primary process");
        match inner_main() {
            Ok(_) => (),
            Err(e) => handle_toplevel_error(e),
        }
    }
}

fn handle_toplevel_error(err: anyhow::Error) {
    if let Some(e) = err.downcast_ref::<InquireError>() {
        match e {
            InquireError::OperationCanceled
            | InquireError::OperationInterrupted
            | InquireError::NotTTY => eprintln!("{e}"),
            _ => panic!("{err}"),
        }
    } else {
        panic!("{err}");
    }
}

#[tokio::main]
async fn inner_main() -> anyhow::Result<()> {
    let args = Args::parse();
    let args = match args.command {
        Command::Burn(a) => a,
    };

    let compression = ask_compression(&args)?;

    let target = match &args.out {
        Some(f) => {
            let dev = BurnTarget::try_from(f.as_ref())?;
            if !confirm_write(&args, compression, &dev)? {
                eprintln!("Aborting.");
                return Ok(());
            }
            dev
        }
        None => ask_outfile(&args, compression)?,
    };

    let burn_args = BurnConfig {
        dest: target.devnode.clone(),
        src: args.input.to_owned(),
        logfile: get_log_paths().child.clone(),
        verify: true,
        compression,
        target_type: target.target_type,
    };

    let handle = try_start_burn(&burn_args).await?;

    begin_writing(target, handle, compression, &args).await?;

    debug!("Done!");
    Ok(())
}

async fn try_start_burn(args: &BurnConfig) -> anyhow::Result<burn::Handle> {
    match burn::Handle::start(args, false).await {
        Ok(p) => Ok(p),
        Err(e) => {
            if let Some(dc) = e.downcast_ref::<StartProcessError>() {
                match dc {
                    StartProcessError::Failed(Some(ErrorType::PermissionDenied)) => {
                        debug!("Failure due to insufficient perms, asking user to escalate");

                        let response = Confirm::new(&format!(
                            "We don't have permissions on {}. Escalate using sudo?",
                            args.dest.to_string_lossy()
                        ))
                        .with_help_message(
                            "We will use the sudo command, which may prompt you for a password.",
                        )
                        .prompt()?;

                        if response {
                            return burn::Handle::start(args, true).await;
                        }
                    }
                    _ => (),
                }
            }
            return Err(e);
        }
    }
}

async fn begin_writing(
    target: BurnTarget,
    handle: burn::Handle,
    cf: CompressionFormat,
    args: &BurnArgs,
) -> anyhow::Result<()> {
    debug!("Opening TUI");
    let mut tui = TUICapture::new()?;
    let terminal = tui.terminal();

    // create app and run it
    ui::burn::UI::new(handle, terminal, target, cf, args)
        .show()
        .await?;

    debug!("Closing TUI");

    Ok(())
}
