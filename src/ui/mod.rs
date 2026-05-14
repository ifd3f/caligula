pub mod cli;
mod fancy_ui;
mod simple_ui;
mod utils;

use std::{fs::File, sync::Arc};

use tracing::{debug, info};

pub use self::{cli::BurnArgs, utils::ByteSpeed};
use crate::{
    facade::CaligulaFacade,
    logging::LogPaths,
    runtime::RemoteSpawn,
    tty::TermiosRestore,
    ui::{simple_ui::do_setup_wizard, utils::TUICapture},
};

/// Entrypoint for both TUI-based UIs.
pub fn main(
    runtime: impl RemoteSpawn,
    facade: Arc<impl CaligulaFacade>,
    log_paths: Arc<LogPaths>,
    args: BurnArgs,
) -> anyhow::Result<()> {
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

    let Some(start_write_verify) = do_setup_wizard(facade.as_ref(), &args)? else {
        return Ok(());
    };

    let child_state = simple_ui::try_start_write_or_escalate(
        facade.clone(),
        &runtime,
        &start_write_verify,
        args.root,
        args.interactive.is_interactive(),
    )?;

    if args.interactive.is_interactive() {
        let mut tui = TUICapture::new()?;
        let terminal = tui.terminal();
        // create app and run it
        fancy_ui::run(
            runtime,
            fancy_ui::Params {
                terminal,
                begin: &start_write_verify,
                child_state,
                terminal_events: crossterm::event::EventStream::new(),
                log_paths: &log_paths,
            },
        );
    } else {
        simple_ui::run(simple_ui::Params {
            child_state,
            log_paths: &log_paths,
        });
    }

    debug!("Done!");
    Ok(())
}
