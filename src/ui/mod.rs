mod cli;
mod fancy_ui;
mod simple_ui;
mod utils;

use std::{fs::File, sync::Arc};

pub use self::cli::BurnArgs;
pub use self::utils::ByteSpeed;
use crate::{
    logging::LogPaths,
    orchestrator::Orchestrator,
    runtime::RemoteSpawn,
    tty::TermiosRestore,
    ui::{simple_ui::do_setup_wizard, utils::TuiManager},
};
use tracing::{debug, info};

/// Entrypoint for both TUI-based UIs.
pub fn main(
    runtime: impl RemoteSpawn,
    orc: Arc<impl Orchestrator + Send + Sync + 'static>,
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

    let Some(start_write_verify) = do_setup_wizard(&args)? else {
        return Ok(());
    };

    let started = simple_ui::try_start_write_or_escalate(
        orc.clone(),
        &runtime,
        &start_write_verify,
        args.root,
        args.interactive.is_interactive(),
    )?;

    if args.interactive.is_interactive() {
        let mut tui = TuiManager::new()?;
        let terminal = tui.terminal();
        // create app and run it
        fancy_ui::run(
            runtime,
            fancy_ui::Params {
                terminal,
                begin: &start_write_verify,
                child_state: started.state,
                terminal_events: crossterm::event::EventStream::new(),
                log_paths: &log_paths,
            },
        );
    } else {
        simple_ui::run(simple_ui::Params {
            child_state: started.state,
            log_paths: &log_paths,
        });
    }

    debug!("Done!");
    Ok(())
}
