use clap::{CommandFactory as _, Parser};
use tracing::debug;

mod byteseries;
mod compression;
mod device;
mod escalation;
mod evdist;
mod hash;
mod hashfile;
mod herder;
mod herder_daemon;
mod ipc_common;
mod logging;
mod native;
mod tty;
mod ui;
mod util;
mod writer_process;

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

    /// INTERNAL ONLY!
    ///
    /// This is a backend entrypoint that is used in implementing automatic root escalation.
    /// There are ZERO stability guarantees. Do NOT rely on this interface for anything.
    #[command(name = "_herder", hide = true)]
    HerderDaemon(HerderDaemonArgs),
}

#[derive(clap::Parser, Debug)]
pub struct HerderDaemonArgs {
    log_file: String,
}

#[tokio::main]
async fn main() {
    let args: Args = match std::env::var("_CALIGULA_CONFIGURE_CLAP_FOR_README") {
        Ok(var) if var == "1" => parse_args_for_readme_generation(),
        _ => Args::parse(),
    };

    match args.command {
        Command::Burn(burn_args) => {
            let state_dir = util::ensure_state_dir().await.unwrap();
            let log_paths = logging::LogPaths::init(&state_dir);
            logging::init_logging_parent(&log_paths);

            debug!("Starting primary process");
            match ui::main(&state_dir, log_paths.into(), &burn_args).await {
                Ok(_) => (),
                Err(e) => handle_toplevel_error(e),
            }
        }
        Command::HerderDaemon(args) => {
            logging::init_logging_child(args.log_file);
            herder_daemon::main().await;
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

/// Parse [Args] from the provided args, but format the help in an easy way for generating
/// the section in the README.md.
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
            // Since this is more of a development-time error, we aren't doing as fancy of a quit
            // as `get_matches`
            e.exit()
        }
    }
}
