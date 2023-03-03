use std::{fmt::Display, io::Stdout};

use bytesize::ByteSize;
use crossterm::{
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use tui::{backend::CrosstermBackend, Terminal};

pub struct TUICapture {
    pub terminal: Terminal<CrosstermBackend<Stdout>>,
    _private: (),
}

impl TUICapture {
    pub fn new() -> anyhow::Result<Self> {
        // setup terminal
        enable_raw_mode()?;
        let mut stdout = std::io::stdout();
        execute!(stdout, EnterAlternateScreen)?;
        let backend = CrosstermBackend::new(stdout);
        let terminal = Terminal::new(backend)?;

        Ok(Self {
            terminal,
            _private: (),
        })
    }
}

impl Drop for TUICapture {
    fn drop(&mut self) {
        // restore terminal
        disable_raw_mode().unwrap();
        execute!(self.terminal.backend_mut(), LeaveAlternateScreen,).unwrap();
        self.terminal.show_cursor().unwrap();
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
