use std::sync::Arc;

use clap::{CommandFactory as _, Parser};
use tracing::debug;

use crate::{
    facade::make_real_facade,
    logging::{ErrorContext, crash_and_burn},
    util::runtime::{AsyncRuntime, RemoteSpawn as _},
};

mod benchmarking;
mod codec;
mod facade;
mod herder_api;
mod herder_daemon;
mod logging;
mod native;
mod tui;
mod util;

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
    Burn(tui::BurnArgs),

    #[command(hide = true)]
    Bench(benchmarking::BenchArgs),

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
            let error_context = logging::init_logging_parent(&log_paths);

            let log_path = log_paths.main().to_owned();
            let runtime = AsyncRuntime::start();

            let facade = Arc::new(
                runtime
                    .spawn(move || async move { make_real_facade(log_path).await })
                    .blocking_recv()
                    .expect("unexpectedly dropped!")
                    .expect("Failed to initialize backend!"),
            );

            debug!("Starting primary process");
            match tui::main(runtime, facade, log_paths.into(), burn_args) {
                Ok(_) => (),
                Err(e) => handle_toplevel_error(&error_context, e),
            }
        }
        Command::HerderDaemon(args) => {
            logging::init_logging_child(args.log_file);
            herder_daemon::main();
        }
        Command::Bench(args) => crate::benchmarking::main(args),
    }
}

fn handle_toplevel_error(ctx: &ErrorContext, err: anyhow::Error) {
    use inquire::InquireError;

    if let Some(e) = err.downcast_ref::<InquireError>() {
        match e {
            // These "errors" are normal exit statuses
            InquireError::OperationCanceled
            | InquireError::OperationInterrupted
            | InquireError::NotTTY => {
                eprintln!("{e}");
                return;
            }
            _ => (), // fallthrough to crash and burn
        }
    }

    crash_and_burn(ctx, err);
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
