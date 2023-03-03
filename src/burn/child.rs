use std::{
    env,
    fs::{File, OpenOptions},
    io::{self, Read, Seek, Write},
    path::PathBuf,
    time::Instant,
};

use bytesize::ByteSize;
use interprocess::local_socket::LocalSocketStream;
use tracing::info;

use super::{ipc::*, BURN_ENV};

pub fn is_in_burn_mode() -> bool {
    env::var(BURN_ENV) == Ok("1".to_string())
}

/// This is intended to be run in a forked child process, possibly with
/// escalated permissions.
pub fn main() {
    let cli_args: Vec<String> = env::args().collect();
    let args = serde_json::from_str(&cli_args[2]).unwrap();
    let pipe = LocalSocketStream::connect(PathBuf::from(&cli_args[1])).unwrap();
    let mut ctx = Ctx { args, pipe };

    let result = match ctx.run() {
        Ok(_) => TerminateResult::Success,
        Err(r) => r,
    };
    ctx.send_msg(StatusMessage::Terminate(result)).unwrap();
}

struct Ctx {
    pipe: LocalSocketStream,
    args: BurnConfig,
}

impl Ctx {
    fn run(&mut self) -> Result<(), TerminateResult> {
        info!("Running child process");

        let mut src = File::open(&self.args.src).unwrap();
        let size = src.seek(io::SeekFrom::End(0))?;
        src.seek(io::SeekFrom::Start(0))?;

        self.burn(&mut src, size)?;

        src.seek(io::SeekFrom::Start(0))?;

        Ok(())
    }

    fn burn(&mut self, src: &mut File, input_file_bytes: u64) -> Result<(), TerminateResult> {
        let mut dest = OpenOptions::new().write(true).open(&self.args.dest)?;

        self.send_msg(StatusMessage::InitSuccess(InitialInfo { input_file_bytes }))?;

        let block_size = ByteSize::kb(128).as_u64() as usize;
        let mut full_block = vec![0u8; block_size];

        let mut written_bytes: usize = 0;

        let stat_checkpoints: usize = 128;
        let checkpoint_blocks: usize = 128;

        loop {
            let start = Instant::now();
            for _ in 0..stat_checkpoints {
                for _ in 0..checkpoint_blocks {
                    let read_bytes = src.read(&mut full_block)?;
                    if read_bytes == 0 {
                        return Ok(());
                    }

                    let write_bytes = dest.write(&full_block[..read_bytes])?;
                    written_bytes += write_bytes;
                    if written_bytes == 0 {
                        return Err(TerminateResult::EndOfOutput);
                    }
                    dest.flush()?;
                }

                self.send_msg(StatusMessage::TotalBytes(written_bytes))?;
            }

            let duration = Instant::now().duration_since(start);
            self.send_msg(StatusMessage::BlockSizeSpeedInfo {
                blocks_written: checkpoint_blocks,
                block_size,
                duration_millis: duration.as_millis() as u64,
            })?;
        }
    }

    fn send_msg(&mut self, msg: StatusMessage) -> Result<(), serde_json::Error> {
        serde_json::to_writer(&mut self.pipe, &msg)?;
        self.pipe.write(b"\n").unwrap();
        Ok(())
    }
}
