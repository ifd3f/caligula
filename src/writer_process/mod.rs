//! This module has logic for the child process that writes to the disk.
//!
//! IT IS NOT TO BE USED DIRECTLY BY THE USER! ITS API HAS NO STABILITY GUARANTEES!

use std::io::BufReader;
use std::{
    fs::File,
    io::{self, Read, Seek, Write},
};

use aligned_vec::avec_rt;
use bytesize::ByteSize;
use interprocess::local_socket::{prelude::*, GenericFilePath};
use tracing::{debug, info};
use tracing_unwrap::ResultExt;

use crate::childproc_common::child_init;
use crate::compression::{decompress, CompressionFormat};
use crate::device;
use crate::ipc_common::write_msg;

use crate::writer_process::utils::{CountRead, CountWrite};
use crate::writer_process::xplat::open_blockdev;

use ipc::*;

pub mod ipc;
#[cfg(test)]
mod tests;
mod utils;
mod xplat;

/// This is intended to be run in a forked child process, possibly with
/// escalated permissions.
pub fn main() {
    let (sock, args) = child_init::<WriterProcessConfig>();

    info!("Opening socket {sock}");
    let mut stream =
        LocalSocketStream::connect(sock.to_fs_name::<GenericFilePath>().unwrap_or_log())
            .unwrap_or_log();

    let mut tx = move |msg: StatusMessage| {
        write_msg(&mut stream, &msg).expect("Failed to write message");
        stream.flush().expect("Failed to flush stream");
    };

    let final_msg = match run(&mut tx, &args) {
        Ok(_) => StatusMessage::Success,
        Err(e) => StatusMessage::Error(e),
    };

    info!(?final_msg, "Completed");
    tx(final_msg);
}

fn run(mut tx: impl FnMut(StatusMessage), args: &WriterProcessConfig) -> Result<(), ErrorType> {
    debug!("Opening file {}", args.src.to_string_lossy());
    let mut file = File::open(&args.src).unwrap_or_log();
    let size = file.seek(io::SeekFrom::End(0))?;
    file.seek(io::SeekFrom::Start(0))?;

    debug!(size, "Got input file size");

    debug!("Opening {} for writing", args.dest.to_string_lossy());

    let mut disk = match args.target_type {
        device::Type::File => File::create(&args.dest)?,
        device::Type::Disk | device::Type::Partition => {
            open_blockdev(&args.dest, args.compression)?
        }
    };

    tx(StatusMessage::InitSuccess(InitialInfo {
        input_file_bytes: size,
    }));
    let buf_size = ByteSize::kib(512).as_u64() as usize;

    WriteOp {
        file: &mut file,
        disk: &mut disk,
        cf: args.compression,
        buf_size,
        disk_block_size: 512,
        checkpoint_period: 32,
        file_read_buf_size: 8192, // default BufReader size
    }
    .execute(&mut tx)?;

    tx(StatusMessage::FinishedWriting {
        verifying: args.verify,
    });

    if !args.verify {
        return Ok(());
    }

    file.seek(io::SeekFrom::Start(0))?;
    disk.seek(io::SeekFrom::Start(0))?;

    VerifyOp {
        file: &mut file,
        disk: &mut disk,
        cf: args.compression,
        buf_size,
        disk_block_size: 512,
        checkpoint_period: 32,
        file_read_buf_size: 8192, // default BufReader size
    }
    .execute(tx)?;

    Ok(())
}

/// Wraps a bunch of parameters for a big complicated operation where we:
/// - decompress the input file
/// - write to a disk
/// - write stats down a pipe
struct WriteOp<F: Read, D: Write> {
    file: F,
    disk: D,
    cf: CompressionFormat,
    buf_size: usize,
    disk_block_size: usize,
    checkpoint_period: usize,
    file_read_buf_size: usize,
}

impl<S: Read, D: Write> WriteOp<S, D> {
    fn execute(&mut self, mut tx: impl FnMut(StatusMessage)) -> Result<(), ErrorType> {
        let mut file = decompress(
            self.cf,
            BufReader::with_capacity(self.file_read_buf_size, CountRead::new(&mut self.file)),
        )
        .unwrap();
        let mut disk = CountWrite::new(&mut self.disk);
        let mut buf = avec_rt![[self.disk_block_size] | 0u8; self.buf_size];

        macro_rules! checkpoint {
            () => {
                disk.flush()?;
                tx(StatusMessage::TotalBytes {
                    src: file.get_mut().get_ref().count(),
                    dest: disk.count(),
                });
            };
        }

        loop {
            for _ in 0..self.checkpoint_period {
                // Try to fill up the block if we can.
                let read_bytes = try_read_exact(&mut file, &mut buf)?;
                if read_bytes == 0 {
                    checkpoint!();
                    return Ok(());
                }

                // Write the entire buffer, because we're doing direct writes.
                // Even if we didn't fill the whole buffer, we are still writing the whole
                // buffer.
                let written_bytes = disk.write(&buf[..])?;
                if written_bytes == 0 {
                    checkpoint!();
                    return Err(ErrorType::EndOfOutput);
                }
            }
            checkpoint!();
        }
    }
}

/// Like [`ReadExt::read_exact`], but if it can't fill the entire buffer, it does not error.
#[inline(always)]
fn try_read_exact(r: &mut impl Read, mut buf: &mut [u8]) -> std::io::Result<usize> {
    // modified from rust stdlib file src/io/mod.rs

    let orig_len = buf.len();
    while !buf.is_empty() {
        match r.read(buf) {
            Ok(0) => break,
            Ok(n) => {
                buf = &mut buf[n..];
            }
            Err(e) => return Err(e),
        }
    }
    Ok(orig_len - buf.len())
}

/// Wraps a bunch of parameters for a big complicated operation where we:
/// - decompress the input file
/// - read from a disk
/// - verify both sides are correct
/// - write stats down a pipe
struct VerifyOp<F: Read, D: Read> {
    file: F,
    disk: D,
    cf: CompressionFormat,
    buf_size: usize,
    disk_block_size: usize,
    checkpoint_period: usize,
    file_read_buf_size: usize,
}

impl<F: Read, D: Read> VerifyOp<F, D> {
    fn execute(&mut self, mut tx: impl FnMut(StatusMessage)) -> Result<(), ErrorType> {
        let mut file = decompress(
            self.cf,
            BufReader::with_capacity(self.file_read_buf_size, CountRead::new(&mut self.file)),
        )
        .unwrap();
        let mut disk = CountRead::new(&mut self.disk);

        let mut file_buf = avec_rt![[self.disk_block_size] | 0u8; self.buf_size];
        let mut disk_buf = avec_rt![[self.disk_block_size] | 0u8; self.buf_size];

        macro_rules! checkpoint {
            () => {
                tx(StatusMessage::TotalBytes {
                    src: file.get_mut().get_ref().count(),
                    dest: disk.count(),
                });
            };
        }

        loop {
            for _ in 0..self.checkpoint_period {
                let read_bytes = file.read(&mut file_buf)?;
                if read_bytes == 0 {
                    checkpoint!();
                    return Ok(());
                }

                disk.read(&mut disk_buf)?;

                if &file_buf[..read_bytes] != &disk_buf[..read_bytes] {
                    return Err(ErrorType::VerificationFailed);
                }
            }
            checkpoint!();
        }
    }
}
