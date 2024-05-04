use std::io::BufReader;
use std::panic::set_hook;
use std::{
    env,
    fs::File,
    io::{self, Read, Seek, Write},
};

use bytesize::ByteSize;
use interprocess::local_socket::{prelude::*, GenericFilePath};
use tracing::{debug, error, info, trace};
use tracing_unwrap::ResultExt;

use crate::compression::decompress;
use crate::device;
use crate::ipc_common::write_msg;
use crate::logging::init_logging_child;

use crate::writer_process::xplat::open_blockdev;

use super::ipc::*;

/// This is intended to be run in a forked child process, possibly with
/// escalated permissions.
#[tokio::main]
pub async fn main() {
    let cli_args: Vec<String> = env::args().collect();
    let args = serde_json::from_str::<WriterProcessConfig>(&cli_args[1]).unwrap_or_log();
    init_logging_child(&args.logfile);

    set_hook(Box::new(|p| {
        error!("{p:?}");
    }));

    info!("We are in child process mode with args {:#?}", args);

    let sock = cli_args[2].as_str();
    info!("Opening socket {sock}");
    let mut stream =
        LocalSocketStream::connect(sock.to_fs_name::<GenericFilePath>().unwrap_or_log())
            .unwrap_or_log();

    let final_msg = match run(&mut stream, &args) {
        Ok(_) => StatusMessage::Success,
        Err(e) => StatusMessage::Error(e),
    };

    info!(?final_msg, "Completed");
    send_msg(stream, final_msg);
}

fn run(mut tx: impl Write, args: &WriterProcessConfig) -> Result<(), ErrorType> {
    debug!("Opening file {}", args.src.to_string_lossy());
    let mut src = File::open(&args.src).unwrap_or_log();
    let size = src.seek(io::SeekFrom::End(0))?;
    src.seek(io::SeekFrom::Start(0))?;

    debug!(size, "Got input file size");

    write(&mut tx, args, &mut src, size)?;
    send_msg(
        &mut tx,
        StatusMessage::FinishedWriting {
            verifying: args.verify,
        },
    );

    if !args.verify {
        return Ok(());
    }

    src.seek(io::SeekFrom::Start(0))?;
    verify(tx, args, &mut src)?;

    Ok(())
}

fn write(
    mut tx: impl Write,
    args: &WriterProcessConfig,
    src: &mut File,
    input_file_bytes: u64,
) -> Result<(), ErrorType> {
    debug!("Opening {} for writing", args.dest.to_string_lossy());

    let file = match args.target_type {
        device::Type::File => File::create(&args.dest)?,
        device::Type::Disk | device::Type::Partition => {
            open_blockdev(&args.dest, args.compression)?
        }
    };
    send_msg(
        &mut tx,
        StatusMessage::InitSuccess(InitialInfo { input_file_bytes }),
    );

    for_each_block(&mut tx, args, src, WriteSink { file })
}

fn verify(tx: impl Write, args: &WriterProcessConfig, src: &mut File) -> Result<(), ErrorType> {
    debug!("Opening {} for verification", args.dest.to_string_lossy());

    let file = File::open(&args.dest)?;
    for_each_block(tx, args, src, VerifySink { file })
}

#[inline]
fn for_each_block(
    mut tx: impl Write,
    args: &WriterProcessConfig,
    src: impl Read + Seek,
    mut sink: impl BlockSink,
) -> Result<(), ErrorType> {
    let block_size = ByteSize::kb(512).as_u64() as usize;
    let mut read_block = vec![0u8; block_size];
    let mut scratch_block = vec![0u8; block_size]; // A block for the user to mutate

    let mut decompress = decompress(args.compression, BufReader::new(src)).unwrap();

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
        send_msg(
            &mut tx,
            StatusMessage::TotalBytes {
                src: decompress.get_mut().stream_position()?,
                dest: offset,
            },
        );
    }

    sink.on_checkpoint()?;
    send_msg(
        tx,
        StatusMessage::TotalBytes {
            src: decompress.get_mut().stream_position()?,
            dest: offset,
        },
    );

    Ok(())
}

#[inline]
pub fn send_msg(mut tx: impl Write, msg: StatusMessage) {
    write_msg(&mut tx, &msg).expect("Failed to write message");
    tx.flush().expect("Failed to flush stream");
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

    use crate::writer_process::{child::VerifySink, ipc::ErrorType};

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
