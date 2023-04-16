use crate::{
    device::BurnTarget,
    logging::init_logging_parent,
    ui::{
        ask_hash::ask_hash,
        ask_outfile,
        burn::start::{begin_writing, try_start_burn, BeginParams},
        cli::{Args, Command},
    },
};
use ask_outfile::{ask_compression, confirm_write};
use clap::Parser;
use inquire::InquireError;
use tracing::debug;

#[tokio::main]
pub async fn main() {
    init_logging_parent();

    debug!("Starting primary process");
    match inner_main().await {
        Ok(_) => (),
        Err(e) => handle_toplevel_error(e),
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

async fn inner_main() -> anyhow::Result<()> {
    let args = Args::parse();
    let args = match args.command {
        Command::Burn(a) => a,
    };

    let compression = ask_compression(&args)?;

    let _hash_info = ask_hash(&args, compression)?;

    let target = match &args.out {
        Some(f) => BurnTarget::try_from(f.as_ref())?,
        None => ask_outfile(&args)?,
    };

    let begin_params = BeginParams::new(args.input.clone(), compression, target)?;
    if !confirm_write(&args, &begin_params)? {
        eprintln!("Aborting.");
        return Ok(());
    }

    let handle = try_start_burn(
        &begin_params.make_child_config(),
        args.root,
        args.interactive.is_interactive(),
    )
    .await?;
    begin_writing(args.interactive, begin_params, handle).await?;

    debug!("Done!");
    Ok(())
}
