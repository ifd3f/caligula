use crate::{
    logging::init_logging_parent,
    ui::{
        ask_outfile,
        burn::start::{begin_writing, try_start_burn, BeginParams},
    },
};
use ask_outfile::ask_compression;
use burn::child::is_in_burn_mode;
use clap::Parser;
use cli::{Args, Command};
use device::BurnTarget;
use inquire::InquireError;
use tracing::debug;
use ui::confirm_write;

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
        Some(f) => BurnTarget::try_from(f.as_ref())?,
        None => ask_outfile(&args)?,
    };

    let begin_params = BeginParams::new(args.input.clone(), compression, target)?;
    if !confirm_write(&args, &begin_params)? {
        eprintln!("Aborting.");
        return Ok(());
    }

    let handle = try_start_burn(&begin_params.make_child_config()).await?;
    begin_writing(begin_params, handle).await?;

    debug!("Done!");
    Ok(())
}
