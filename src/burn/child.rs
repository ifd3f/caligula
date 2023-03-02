use std::{
    env,
    fs::{File, OpenOptions},
    io::{Read, Write},
    time::Instant,
};

use bytesize::ByteSize;

use super::{ipc::*, BURN_ENV};

pub fn is_in_burn_mode() -> bool {
    env::var(BURN_ENV) == Ok("1".to_string())
}

/// This is intended to be run in a forked child process, possibly with
/// escalated permissions.
pub fn main() {
    let result = match run() {
        Ok(r) => r,
        Err(r) => r,
    };
    send_msg(StatusMessage::Terminate(result));
}

fn run() -> Result<TerminateResult, TerminateResult> {
    let args: BurnConfig = bincode::deserialize_from(std::io::stdin()).unwrap();

    let mut src = File::open(&args.src)?;
    let mut dest = OpenOptions::new().write(true).open(&args.dest)?;

    send_msg(StatusMessage::FileOpenSuccess);

    let block_size = ByteSize::kb(128).as_u64() as usize;
    let mut full_block = vec![0; block_size];

    let mut written_bytes: usize = 0;

    let stat_checkpoints: usize = 128;
    let checkpoint_blocks: usize = 128;

    loop {
        let start = Instant::now();
        for _ in 0..stat_checkpoints {
            for _ in 0..checkpoint_blocks {
                let read_bytes = src.read(&mut full_block)?;
                if read_bytes == 0 {
                    return Ok(TerminateResult::EndOfInput);
                }

                let write_bytes = dest.write(&full_block[..read_bytes])?;
                written_bytes += write_bytes;
                if written_bytes == 0 {
                    return Ok(TerminateResult::EndOfOutput);
                }
                dest.flush()?;
            }

            send_msg(StatusMessage::TotalBytesWritten(written_bytes));
        }

        let duration = Instant::now().duration_since(start);
        send_msg(StatusMessage::BlockSizeSpeedInfo {
            blocks_written: checkpoint_blocks,
            block_size,
            duration,
        });
    }
}

fn send_msg(msg: StatusMessage) {
    bincode::serialize_into(std::io::stdout(), &msg);
}
