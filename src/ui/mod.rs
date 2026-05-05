mod cli;
mod fancy_ui;
mod simple_ui;
mod start;
mod utils;

use std::{fs::File, path::Path, sync::Arc};

pub use self::cli::BurnArgs;
pub use self::utils::ByteSpeed;
use crate::{
    logging::LogPaths,
    orchestrator::make_orchestrator_impl,
    tty::TermiosRestore,
    ui::{
        simple_ui::do_setup_wizard,
        start::{begin_writing, try_start_burn},
    },
};
use tracing::{debug, info};

pub async fn main(
    _state_dir: &Path,
    log_paths: Arc<LogPaths>,
    args: &BurnArgs,
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

    let Some(begin_params) = do_setup_wizard(&args)? else {
        return Ok(());
    };

    let mut orc = make_orchestrator_impl(log_paths.main());
    let handle = try_start_burn(
        &mut orc,
        &begin_params,
        args.root,
        args.interactive.is_interactive(),
    )
    .await?;
    begin_writing(args.interactive, begin_params, handle, log_paths).await?;

    debug!("Done!");
    Ok(())
}
