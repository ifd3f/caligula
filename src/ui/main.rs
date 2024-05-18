use std::{fs::File, path::PathBuf, sync::Arc};

use crate::{
    logging::{init_logging_parent, LogPaths},
    tty::TermiosRestore,
    ui::{
        cli::{Args, Command},
        herder::{Herder, HerderSocket},
        simple_ui::do_setup_wizard,
        start::{begin_writing, try_start_burn},
    },
    util::ensure_state_dir,
};
use clap::Parser;
use inquire::InquireError;
use tracing::{debug, info};

#[tokio::main]
pub async fn main() {
    let state_dir = ensure_state_dir().await.unwrap();
    let log_paths = LogPaths::init(&state_dir);
    init_logging_parent(&log_paths);

    let _termios_restore = match File::open("/dev/tty") {
        Ok(tty) => TermiosRestore::new(tty).ok(),
        Err(error) => {
            info!(
                ?error,
                "failed to open /dev/tty, will not attempt to restore after program"
            );
            None
        }
    };

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

    let Some(begin_params) = do_setup_wizard(&args)? else {
        return Ok(());
    };

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
