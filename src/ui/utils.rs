use std::{
    fmt::Display,
    io::Stdout,
    panic::{set_hook, take_hook, PanicInfo},
    sync::Arc,
};

use bytesize::ByteSize;
use crossterm::{
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use tokio::sync::Mutex;
use tracing_unwrap::ResultExt;
use tui::{backend::CrosstermBackend, Terminal};

type PanicHook = Box<dyn Fn(&PanicInfo<'_>) + 'static + Sync + Send>;

pub struct TUICapture {
    terminal: Arc<Mutex<Terminal<CrosstermBackend<Stdout>>>>,
    prev_hook: Arc<PanicHook>,
}

impl TUICapture {
    pub fn new() -> anyhow::Result<Self> {
        // setup terminal
        enable_raw_mode()?;
        let mut stdout = std::io::stdout();
        execute!(stdout, EnterAlternateScreen)?;
        let backend = CrosstermBackend::new(stdout);
        let terminal = Arc::new(Mutex::new(Terminal::new(backend)?));

        let prev_hook = Arc::new(take_hook());

        set_hook({
            let terminal = terminal.clone();
            let prev_hook = prev_hook.clone();
            Box::new(move |p| {
                {
                    let mut lock = terminal.blocking_lock();
                    restore(&mut lock);
                }
                (prev_hook)(p)
            })
        });

        Ok(Self {
            terminal,
            prev_hook,
        })
    }

    pub fn terminal(&self) -> Arc<Mutex<Terminal<CrosstermBackend<Stdout>>>> {
        self.terminal.clone()
    }
}

impl Drop for TUICapture {
    fn drop(&mut self) {
        {
            let mut lock = self.terminal.blocking_lock();
            restore(&mut lock);
        }
        let hook = self.prev_hook.clone();
        set_hook(Box::new(move |p| hook(p)));
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, PartialOrd)]
pub struct ByteSpeed(pub f64);

impl Display for ByteSpeed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let bytes = ByteSize::b(self.0 as u64);
        write!(f, "{bytes}/s")
    }
}

fn restore(terminal: &mut Terminal<CrosstermBackend<Stdout>>) {
    // restore terminal
    disable_raw_mode().unwrap_or_log();
    execute!(terminal.backend_mut(), LeaveAlternateScreen,).unwrap_or_log();
    terminal.show_cursor().unwrap_or_log();
}
