/**
 * Copyright Conrad Kleinespel (https://github.com/conradkleinespel)
 *
 * This code is lifted and modified from the rpassword crate. It is
 * Licensed under APACHE-2.0.
 */
use libc::{c_int, tcsetattr, termios, ECHO, ECHONL, TCSANOW};
use std::io::{self, BufRead};
use std::mem;
use std::os::unix::io::AsRawFd;

pub struct HiddenInput<F: AsRawFd> {
    term_orig: termios,
    file: F,
}

impl<F: AsRawFd> HiddenInput<F> {
    pub fn new(file: F) -> io::Result<HiddenInput<F>> {
        let fd = file.as_raw_fd();

        // Make two copies of the terminal settings. The first one will be modified
        // and the second one will act as a backup for when we want to set the
        // terminal back to its original state.
        let mut term = safe_tcgetattr(fd)?;
        let term_orig = safe_tcgetattr(fd)?;

        // Hide the password. This is what makes this function useful.
        term.c_lflag &= !ECHO;

        // But don't hide the NL character when the user hits ENTER.
        term.c_lflag |= ECHONL;

        // Save the settings for now.
        io_result(unsafe { tcsetattr(fd, TCSANOW, &term) })?;

        Ok(HiddenInput { file, term_orig })
    }
}

impl<F: AsRawFd> Drop for HiddenInput<F> {
    fn drop(&mut self) {
        // Set the the mode back to normal
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
