use std::{path::PathBuf, sync::Arc};

use crate::{
    device::WriteTarget,
    logging::{init_logging_parent, LogPaths},
    ui::{
        ask_hash::ask_hash,
        ask_outfile,
        burn::start::{begin_writing, try_start_burn, BeginParams},
        cli::{Args, Command},
        herder::{Herder, HerderSocket},
    },
    util::ensure_state_dir,
};
use ask_outfile::{ask_compression, confirm_write};
use clap::Parser;
use inquire::InquireError;
use tracing::debug;

#[tokio::main]
pub async fn main() {
    let state_dir = ensure_state_dir().await.unwrap();
    let log_paths = LogPaths::init(&state_dir);
    init_logging_parent(&log_paths);

    debug!("Starting primary process");
    match inner_main(state_dir, log_paths).await {
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

async fn inner_main(state_dir: PathBuf, log_paths: LogPaths) -> anyhow::Result<()> {
    let args = Args::parse();
    let args = match args.command {
        Command::Burn(a) => a,
    };

    let log_paths = Arc::new(log_paths);

    let compression = ask_compression(&args)?;

    let _hash_info = ask_hash(&args, compression)?;

    let target = match &args.out {
        Some(f) => WriteTarget::try_from(f.as_ref())?,
        None => ask_outfile(&args)?,
    };

    let begin_params = BeginParams::new(args.input.clone(), compression, target)?;
    if !confirm_write(&args, &begin_params)? {
        eprintln!("Aborting.");
        return Ok(());
    }

    let socket = HerderSocket::new(state_dir).await?;
    let mut herder = Herder::new(socket, log_paths.clone());
    let handle = try_start_burn(
        &mut herder,
        &begin_params.make_child_config(),
        args.root,
        args.interactive.is_interactive(),
    )
    .await?;
    begin_writing(args.interactive, begin_params, handle, log_paths).await?;

    debug!("Done!");
    Ok(())
}
