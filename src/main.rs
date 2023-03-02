use std::{fs::File, io, time::Duration};

use crate::ui::ask_outfile;
use burn::child::is_in_burn_mode;
use clap::Parser;
use cli::Args;
use crossterm::{
    event::{DisableMouseCapture, EnableMouseCapture},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use device::BurnTarget;
use tui::{backend::CrosstermBackend, Terminal};
use ui::{burn::BurningDisplay, confirm_write, fopen::open_or_escalate};

pub mod burn;
pub mod cli;
mod device;
mod ui;

fn main() {
    if is_in_burn_mode() {
        burn::child::main();
    } else {
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

    let in_file = File::open(&args.input)?;
    let out_dev = open_or_escalate(target.devnode)?;

    let writing = BurnThread::new(out_dev, in_file).start_write()?;
    begin_writing(writing, &args).await?;

    Ok(())
}

async fn begin_writing(writing: burn::Writing, args: &Args) -> anyhow::Result<()> {
    // setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // create app and run it
    BurningDisplay::new(writing, args, &mut terminal)
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
