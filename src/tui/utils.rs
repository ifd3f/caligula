use std::{fmt::Display, io::Stdout, mem, os::fd::AsRawFd};

use bytesize::ByteSize;
use crossterm::{
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use libc::{TCSANOW, c_int, tcsetattr, termios};
use ratatui::{Terminal, backend::CrosstermBackend};
use tracing::info;
use tracing_unwrap::ResultExt;

pub struct TUICapture {
    terminal: Terminal<CrosstermBackend<Stdout>>,
    _private: (),
}

impl TUICapture {
    pub fn new() -> std::io::Result<Self> {
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

    pub fn terminal(&mut self) -> &mut Terminal<CrosstermBackend<Stdout>> {
        &mut self.terminal
    }
}

impl Drop for TUICapture {
    fn drop(&mut self) {
        // restore terminal
        disable_raw_mode().unwrap_or_log();
        execute!(self.terminal.backend_mut(), LeaveAlternateScreen,).unwrap_or_log();
        self.terminal.show_cursor().unwrap_or_log();
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

/// Stores the state of the terminal when created, and restores it on drop
pub struct TermiosRestore<F: AsRawFd> {
    term_orig: termios,
    file: F,
}

impl<F: AsRawFd> TermiosRestore<F> {
    pub fn new(file: F) -> std::io::Result<TermiosRestore<F>> {
        info!("attempting to store terminal state before program started");
        let fd = file.as_raw_fd();
        let term_orig = safe_tcgetattr(fd)?;
        Ok(TermiosRestore { file, term_orig })
    }
}

impl<F: AsRawFd> Drop for TermiosRestore<F> {
    fn drop(&mut self) {
        info!("restoring terminal state to what it was before program started");
        unsafe {
            tcsetattr(self.file.as_raw_fd(), TCSANOW, &self.term_orig);
        }
    }
}

/// Turns a C function return into an IO Result
fn io_result(ret: c_int) -> std::io::Result<()> {
    match ret {
        0 => Ok(()),
        _ => Err(std::io::Error::last_os_error()),
    }
}

fn safe_tcgetattr(fd: c_int) -> std::io::Result<termios> {
    let mut term = mem::MaybeUninit::<termios>::uninit();
    io_result(unsafe { ::libc::tcgetattr(fd, term.as_mut_ptr()) })?;
    Ok(unsafe { term.assume_init() })
}
