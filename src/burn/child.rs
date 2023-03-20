use std::io::BufReader;
use std::panic::set_hook;
use std::{
    env,
    fs::File,
    io::{self, Read, Seek, Write},
    path::PathBuf,
};

use bytesize::ByteSize;
use interprocess::local_socket::LocalSocketStream;
use tracing::{debug, error, info, trace, trace_span};
use tracing_unwrap::ResultExt;
use valuable::Valuable;

use crate::compression::decompress;
use crate::device;
use crate::logging::init_logging_child;

use crate::burn::xplat::open_blockdev;

use super::{ipc::*, BURN_ENV};

pub fn is_in_burn_mode() -> bool {
    env::var(BURN_ENV) == Ok("1".to_string())
}

/// This is intended to be run in a forked child process, possibly with
/// escalated permissions.
pub fn main() {
    let cli_args: Vec<String> = env::args().collect();
    let args = serde_json::from_str::<BurnConfig>(&cli_args[1]).unwrap_or_log();
    init_logging_child(&args.logfile);

    set_hook(Box::new(|p| {
        error!("{p}");
    }));

    debug!("We are in child process mode with args {:#?}", args);

    let sock = cli_args[2].as_str();
    info!("Opening socket {sock}");
    let reporter = StatusReporter::open(sock);

    let mut ctx = Ctx { args, reporter };
    let final_msg = match ctx.run() {
        Ok(_) => StatusMessage::Success,
        Err(e) => StatusMessage::Error(e),
    };
    ctx.send_msg(final_msg);
}

struct Ctx {
    reporter: StatusReporter,
    args: BurnConfig,
}

impl Ctx {
    fn run(&mut self) -> Result<(), ErrorType> {
        debug!("Opening file {}", self.args.src.to_string_lossy());
        let mut src = File::open(&self.args.src).unwrap_or_log();
        let size = src.seek(io::SeekFrom::End(0))?;
        src.seek(io::SeekFrom::Start(0))?;

        debug!(size, "Got input file size");

        self.burn(&mut src, size)?;
        self.send_msg(StatusMessage::FinishedWriting {
            verifying: self.args.verify,
        });

        if !self.args.verify {
            return Ok(());
        }

        src.seek(io::SeekFrom::Start(0))?;
        self.verify(&mut src)?;

        Ok(())
    }

    fn burn(&mut self, src: &mut File, input_file_bytes: u64) -> Result<(), ErrorType> {
        debug!("Opening {} for writing", self.args.dest.to_string_lossy());

        let file = match self.args.target_type {
            device::Type::File => File::create(&self.args.dest)?,
            device::Type::Disk | device::Type::Partition => {
                open_blockdev(&self.args.dest, self.args.compression)?
            }
        };
        self.send_msg(StatusMessage::InitSuccess(InitialInfo { input_file_bytes }));

        for_each_block(self, src, WriteSink { file })
    }

    fn verify(&mut self, src: &mut File) -> Result<(), ErrorType> {
        debug!(
            "Opening {} for verification",
            self.args.dest.to_string_lossy()
        );

        let file = File::open(&self.args.dest)?;
        for_each_block(self, src, VerifySink { file })
    }

    pub fn send_msg(&mut self, result: StatusMessage) {
        self.reporter.send_msg(result);
    }
}

#[inline]
fn for_each_block(
    ctx: &mut Ctx,
    src: impl Read + Seek,
    mut sink: impl BlockSink,
) -> Result<(), ErrorType> {
    let block_size = ByteSize::kb(512).as_u64() as usize;
    let mut read_block = vec![0u8; block_size];
    let mut scratch_block = vec![0u8; block_size]; // A block for the user to mutate

    let mut decompress = decompress(ctx.args.compression, BufReader::new(src)).unwrap();

    let checkpoint_blocks: usize = 32;
    let mut offset: u64 = 0;

    'outer: loop {
        for _ in 0..checkpoint_blocks {
            let read_bytes = decompress.read(&mut read_block)?;
            if read_bytes == 0 {
                break 'outer;
            }

            sink.on_block(&read_block[..read_bytes], &mut scratch_block[..read_bytes])?;
            offset += read_bytes as u64;
        }

        sink.on_checkpoint()?;
        ctx.send_msg(StatusMessage::TotalBytes {
            src: decompress.get_mut().stream_position()?,
            dest: offset,
        });
    }

    sink.on_checkpoint()?;
    ctx.send_msg(StatusMessage::TotalBytes {
        src: decompress.get_mut().stream_position()?,
        dest: offset,
    });

    Ok(())
}

struct StatusReporter(LocalSocketStream);

impl StatusReporter {
    fn open(path: &str) -> Self {
        Self(LocalSocketStream::connect(PathBuf::from(path)).unwrap_or_log())
    }

    fn send_msg(&mut self, msg: StatusMessage) {
        let _span = trace_span!("Sending message {:?}", msg = msg.as_value());
        write_msg(&mut self.0, &msg).expect("Failed to write message");
    }
}

trait BlockSink {
    fn on_block(&mut self, block: &[u8], scratch: &mut [u8]) -> Result<(), ErrorType>;
    fn on_checkpoint(&mut self) -> Result<(), ErrorType>;
}

struct WriteSink<W>
where
    W: Write,
{
    file: W,
}

impl<W> BlockSink for WriteSink<W>
where
    W: Write,
{
    #[inline]
    fn on_block(&mut self, block: &[u8], _scratch: &mut [u8]) -> Result<(), ErrorType> {
        trace!(block_len = block.len(), "Writing block");

        let written = self
            .file
            .write(block)
            .expect("Failed to write block to disk");
        if written != block.len() {
            return Err(ErrorType::EndOfOutput);
        }
        Ok(())
    }

    #[inline]
    fn on_checkpoint(&mut self) -> Result<(), ErrorType> {
        self.file.flush()?;
        Ok(())
    }
}

struct VerifySink<R>
where
    R: Read,
{
    file: R,
}

impl<R> BlockSink for VerifySink<R>
where
    R: Read,
{
    #[inline]
    fn on_block(&mut self, block: &[u8], scratch: &mut [u8]) -> Result<(), ErrorType> {
        trace!(block_len = block.len(), "Verifying block");

        let read = self
            .file
            .read(scratch)
            .expect("Failed to read block from disk");
        if read != block.len() {
            return Err(ErrorType::EndOfOutput);
        }
        if block != scratch {
            return Err(ErrorType::VerificationFailed);
        }
        Ok(())
    }

    #[inline]
    fn on_checkpoint(&mut self) -> Result<(), ErrorType> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use rand::{thread_rng, RngCore};

    use crate::burn::{child::VerifySink, ipc::ErrorType};

    use super::{BlockSink, WriteSink};

    fn make_random(n: usize) -> Vec<u8> {
        let mut rng = thread_rng();
        let mut dest = vec![0; n];
        rng.fill_bytes(&mut dest);
        dest
    }

    #[test]
    fn write_sink_on_block() {
        let mut sink = WriteSink { file: vec![] };

        sink.on_block(&[1, 2, 3, 4], &mut make_random(4)).unwrap();
        sink.on_block(&[1, 2, 3, 4, 5, 6], &mut make_random(6))
            .unwrap();

        assert_eq!(sink.file, vec![1, 2, 3, 4, 1, 2, 3, 4, 5, 6]);
    }

    #[test]
    fn verify_sink_multiple_blocks_incorrect() {
        let src = make_random(1000);
        let mut file = src.clone();
        file[593] = 5;

        let mut sink = VerifySink {
            file: Cursor::new(file),
        };

        sink.on_block(&src[..250], &mut make_random(250)).unwrap();
        sink.on_block(&src[250..500], &mut make_random(250))
            .unwrap();
        let r2 = sink
            .on_block(&src[500..750], &mut make_random(250))
            .unwrap_err();

        assert_eq!(r2, ErrorType::VerificationFailed);
    }

    #[test]
    fn verify_sink_multiple_blocks_correct() {
        let src = make_random(1000);
        let file = src.clone();

        let mut sink = VerifySink {
            file: Cursor::new(file),
        };

        sink.on_block(&src[..500], &mut make_random(500)).unwrap();
        sink.on_block(&src[500..], &mut make_random(500)).unwrap();
    }
}
