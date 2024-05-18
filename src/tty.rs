use std::{mem, os::fd::AsRawFd};

use libc::{c_int, tcsetattr, termios, TCSANOW};
use tracing::info;

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
