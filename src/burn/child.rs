use std::{
    env,
    fs::File,
    io::{self, Read, Seek, Write},
    path::PathBuf,
};

use bytesize::ByteSize;
use interprocess::local_socket::LocalSocketStream;
use tracing::{debug, info};
use tracing_unwrap::ResultExt;

use crate::burn::xplat::open_blockdev;

use super::{ipc::*, BURN_ENV};

pub fn is_in_burn_mode() -> bool {
    env::var(BURN_ENV) == Ok("1".to_string())
}

/// This is intended to be run in a forked child process, possibly with
/// escalated permissions.
pub fn main() {
    let cli_args: Vec<String> = env::args().collect();
    let args = serde_json::from_str(&cli_args[1]).unwrap_or_log();

    let pipe = cli_args[2].as_str();
    info!(pipe, "Got args {:#?}", args);

    match pipe {
        "-" => run_with_pipe(args, std::io::stdout()),
        path => run_with_pipe(
            args,
            LocalSocketStream::connect(PathBuf::from(path)).unwrap_or_log(),
        ),
    };

    fn run_with_pipe(args: BurnConfig, pipe: impl Write) {
        let mut ctx = Ctx { args, pipe };
        let result = match ctx.run() {
            Ok(_) => TerminateResult::Success,
            Err(r) => r,
        };
        ctx.send_msg(StatusMessage::Terminate(result))
            .unwrap_or_log();
    }
}

struct Ctx<P>
where
    P: Write,
{
    pipe: P,
    args: BurnConfig,
}

impl<P> Ctx<P>
where
    P: Write,
{
    fn run(&mut self) -> Result<(), TerminateResult> {
        let mut src = File::open(&self.args.src).unwrap_or_log();
        let size = src.seek(io::SeekFrom::End(0))?;
        src.seek(io::SeekFrom::Start(0))?;

        debug!(size, "Got input file size");

        self.burn(&mut src, size)?;
        self.send_msg(StatusMessage::FinishedWriting {
            verifying: self.args.verify,
        })?;

        if !self.args.verify {
            return Ok(());
        }

        src.seek(io::SeekFrom::Start(0))?;
        self.verify(&mut src)?;

        Ok(())
    }

    fn burn(&mut self, src: &mut File, input_file_bytes: u64) -> Result<(), TerminateResult> {
        debug!("Running burn");

        let mut dest = open_blockdev(&self.args.dest)?;
        self.send_msg(StatusMessage::InitSuccess(InitialInfo { input_file_bytes }))?;

        for_each_block(self, src, |block, _| {
            let written = dest.write(block)?;
            if written != block.len() {
                return Err(TerminateResult::EndOfOutput);
            }
            Ok(())
        })
    }

    fn verify(&mut self, src: &mut File) -> Result<(), TerminateResult> {
        debug!("Running verify");

        let mut dest = File::open(&self.args.dest)?;
        for_each_block(self, src, |block, dst| {
            let read = dest.read(dst)?;
            if read != block.len() {
                return Err(TerminateResult::EndOfOutput);
            }
            if block != dst {
                return Err(TerminateResult::VerificationFailed);
            }
            Ok(())
        })
    }

    fn send_msg(&mut self, msg: StatusMessage) -> Result<(), serde_json::Error> {
        debug!("Sending message {:?}", msg);
        serde_json::to_writer(&mut self.pipe, &msg)?;
        self.pipe.write(b"\n").unwrap_or_log();
        Ok(())
    }
}

#[inline]
fn for_each_block(
    ctx: &mut Ctx<impl Write>,
    src: &mut File,
    mut action: impl FnMut(&[u8], &mut [u8]) -> Result<(), TerminateResult>,
) -> Result<(), TerminateResult> {
    let block_size = ByteSize::kb(128).as_u64() as usize;
    let mut full_block = vec![0u8; block_size];
    let mut closure_block = vec![0u8; block_size]; // A block for the user to mutate

    let mut written_bytes: usize = 0;

    let checkpoint_blocks: usize = 32;

    loop {
        for _ in 0..checkpoint_blocks {
            let read_bytes = src.read(&mut full_block)?;
            if read_bytes == 0 {
                return Ok(());
            }

            action(&full_block[..read_bytes], &mut closure_block[..read_bytes])?;
            written_bytes += read_bytes;
        }

        ctx.send_msg(StatusMessage::TotalBytes(written_bytes))?;
    }
}
