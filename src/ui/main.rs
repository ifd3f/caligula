use crate::{
    logging::init_logging_parent,
    ui::{
        ask_input::{ask_compression, ask_hash},
        burn::start::{begin_writing, InputFileParams},
        cli::{Args, Command},
    },
};
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

    let begin_params = InputFileParams::new(args.input.clone(), compression)?;

    begin_writing(&args, begin_params).await?;

    debug!("Done!");
    Ok(())
}
