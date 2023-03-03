use std::io;

use crate::ui::ask_outfile;
use burn::{
    child::is_in_burn_mode,
    handle::StartProcessError,
    ipc::{BurnConfig, TerminateResult},
    Handle,
};
use clap::Parser;
use cli::Args;
use crossterm::{
    event::{DisableMouseCapture, EnableMouseCapture},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use device::BurnTarget;
use inquire::Confirm;
use tracing::{debug, Level};
use tui::{backend::CrosstermBackend, Terminal};
use ui::{burn::BurningDisplay, confirm_write};

pub mod burn;
pub mod cli;
mod device;
mod ui;

fn main() {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_max_level(Level::DEBUG)
        .init();

    if is_in_burn_mode() {
        debug!("We are in child process mode");
        burn::child::main();
    } else {
        debug!("Starting primary process");
        cli_main().unwrap();
    }
}

#[tokio::main]
async fn cli_main() -> anyhow::Result<()> {
    let args = Args::parse();

    let target = match &args.out {
        Some(f) => {
            let dev = BurnTarget::try_from(f.as_ref())?;
            if !confirm_write(&args, &dev)? {
                eprintln!("Aborting.");
                return Ok(());
            }
            dev
        }
        None => ask_outfile(&args)?,
    };

    let burn_args = BurnConfig {
        dest: target.devnode,
        src: args.input.to_owned(),
        mode: cli::BurnMode::Normal,
    };

    let handle = try_start_burn(&burn_args).await?;

    begin_writing(handle, &args).await?;

    Ok(())
}

async fn try_start_burn(args: &BurnConfig) -> anyhow::Result<burn::Handle> {
    match burn::Handle::start(args, false).await {
        Ok(p) => Ok(p),
        Err(e) => {
            if let Some(dc) = e.downcast_ref::<StartProcessError>() {
                match dc {
                    StartProcessError::Failed(Some(TerminateResult::PermissionDenied)) => {
                        let response = Confirm::new(&format!(
                            "We don't have permissions on {}. Escalate using sudo?",
                            args.dest.to_string_lossy()
                        ))
                        .with_help_message(
                            "We will use the sudo command, which may prompt you for a password.",
                        )
                        .prompt()?;

                        if response {
                            return burn::Handle::start(args, true).await;
                        }
                    }
                    _ => (),
                }
            }
            return Err(e);
        }
    }
}

async fn begin_writing(handle: burn::Handle, args: &Args) -> anyhow::Result<()> {
    // setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // create app and run it
    BurningDisplay::new(handle, args, &mut terminal)
        .show()
        .await?;

    // restore terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    Ok(())
}
