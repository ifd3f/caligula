use std::sync::Arc;

use clap::{CommandFactory as _, Parser};
use tracing::debug;

use crate::facade::make_real_facade;

mod byteseries;
mod compression;
mod device;
mod escalation;
mod facade;
mod hash;
mod hashfile;
mod herder_api;
mod herder_daemon;
mod ipc_common;
mod logging;
mod native;
mod runtime;
mod tty;
mod ui;
mod util;

#[cfg(feature = "gui")]
mod gui;

/// A lightweight, user-friendly disk imaging tool
#[derive(clap::Parser, Debug)]
#[command(author, version, about, long_about = None, flatten_help = true)]
#[command(propagate_version = true)]
pub struct Args {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(clap::Subcommand, Debug)]
pub enum Command {
    Burn(ui::BurnArgs),

    #[cfg(feature = "gui")]
    Gui,

    /// INTERNAL ONLY!
    ///
    /// This is a backend entrypoint that is used in implementing automatic root
    /// escalation. There are ZERO stability guarantees. Do NOT rely on this
    /// interface for anything.
    #[command(name = "_herder", hide = true)]
    HerderDaemon(HerderDaemonArgs),
}

#[derive(clap::Parser, Debug)]
pub struct HerderDaemonArgs {
    log_file: String,
}

fn main() {
    let args: Args = match std::env::var("_CALIGULA_CONFIGURE_CLAP_FOR_README") {
        Ok(var) if var == "1" => parse_args_for_readme_generation(),
        _ => Args::parse(),
    };

    match args.command {
        Command::Burn(burn_args) => {
            let state_dir = util::ensure_state_dir().unwrap();
            let log_paths = logging::LogPaths::init(&state_dir);
            logging::init_logging_parent(&log_paths);

            let runtime = crate::runtime::AsyncRuntime::start();
            let facade = Arc::new(make_real_facade(log_paths.main()));

            debug!("Starting primary process");
            match ui::main(runtime, facade, log_paths.into(), burn_args) {
                Ok(_) => (),
                Err(e) => handle_toplevel_error(e),
            }
        }
        #[cfg(feature = "gui")]
        Command::Gui => {
            // FIXME: duplicated setup from `Command::Burn`

            let state_dir = util::ensure_state_dir().unwrap();
            let log_paths = logging::LogPaths::init(&state_dir);
            logging::init_logging_parent(&log_paths);

            let runtime = crate::runtime::AsyncRuntime::start();
            let orc = Arc::new(make_real_facade(log_paths.main()));

            debug!("Starting primary process");
            match gui::main(runtime, orc, log_paths.into()) {
                Ok(_) => (),
                // FIXME: shitty to_string on error
                Err(e) => handle_toplevel_error(anyhow::anyhow!(e.to_string())),
            }
        }
        Command::HerderDaemon(args) => {
            logging::init_logging_child(args.log_file);
            herder_daemon::main();
        }
    }
}

fn handle_toplevel_error(err: anyhow::Error) {
    use inquire::InquireError;

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

/// Parse [Args] from the provided args, but format the help in an easy way for
/// generating the section in the README.md.
fn parse_args_for_readme_generation() -> Args {
    use clap::FromArgMatches;

    let command = Args::command_for_update()
        .color(clap::ColorChoice::Never)
        .term_width(0);

    // The rest of this function is lifted out of clap::Parser::parse().
    let mut matches = command.get_matches();
    let res = Args::from_arg_matches_mut(&mut matches).map_err(|err| {
        let mut cmd = Args::command();
        err.format(&mut cmd)
    });
    match res {
        Ok(s) => s,
        Err(e) => {
            // Since this is more of a development-time error, we aren't doing as fancy of a
            // quit as `get_matches`
            e.exit()
        }
    }
}
