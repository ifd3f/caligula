//! This module has logic for the child process that writes to the disk.
//!
//! IT IS NOT TO BE USED DIRECTLY BY THE USER! ITS API HAS NO STABILITY
//! GUARANTEES!

use std::{
    fs::{File, OpenOptions},
    io::{self, Read, Seek},
    process::{Command, Stdio},
    thread::JoinHandle,
};

use tracing::{debug, info};
use tracing_unwrap::ResultExt;

use crate::{
    herder_api::{
        error::{CommandError, DiskError, InputFileError, IoError, UnmountError},
        write_verify::*,
    },
    util::{
        device,
        legacy_io::{SyncDataFile, VerifyOp, WriteOp, open_blockdev},
    },
};

/// Maximum size we may allocate for each buffer.
const MAX_BUF_SIZE: usize = 1 << 20; // 1MiB

/// How many bytes should be written before we perform a checkpoint (aka report
/// progress).
const CHECKPOINT_BYTES: usize = 8 * (1 << 20); // 8MiB

pub fn spawn_writer(
    tx_start: impl FnOnce(Result<WVStart, WVError>) + Send + 'static,
    mut tx: impl FnMut(Result<WVEvent, WVError>) + Send + 'static,
    init_config: WVAction,
) -> JoinHandle<()> {
    std::thread::Builder::new()
        .name("writer".into())
        .spawn(move || {
            debug!("Spawned child thread {:?}", std::thread::current().id());
            let start = setup(&init_config);
            let (f, d) = match start {
                Ok((f, d, s)) => {
                    tx_start(Ok(s));
                    (f, d)
                }
                Err(e) => {
                    tx_start(Err(e));
                    return;
                }
            };

            let borrowed = &mut tx;
            let result =
                run(f, d, move |x| borrowed(Ok(x)), &init_config).map(|_| WVEvent::Success);

            info!(?result, "Thread terminated");
            tx(result);
        })
        .unwrap()
}

fn setup(args: &WVAction) -> Result<(File, SyncDataFile, WVStart), WVError> {
    if cfg!(target_os = "macos") && args.target_type == device::Type::Disk {
        run_diskutil_umount(args).map_err(UnmountError::Diskutil)?;
    }
    info!("Opening file {}", args.src.to_string_lossy());
    let mut file = File::open(&args.src).unwrap_or_log();
    let size = file
        .seek(io::SeekFrom::End(0))
        .map_err(IoError::<InputFileError>::from)?;
    file.seek(io::SeekFrom::Start(0))
        .map_err(IoError::<InputFileError>::from)?;
    info!(size, "Got input file size");
    info!("Opening {} for writing", args.dest.to_string_lossy());
    let disk = SyncDataFile(match args.target_type {
        device::Type::File => OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(&args.dest)
            .map_err(IoError::<DiskError>::from)?,
        device::Type::Disk | device::Type::Partition => {
            open_blockdev(&args.dest).map_err(IoError::<DiskError>::from)?
        }
    });

    Ok((
        file,
        disk,
        WVStart {
            input_file_bytes: size,
        },
    ))
}

fn run(
    mut file: File,
    mut disk: SyncDataFile,
    mut tx: impl FnMut(WVEvent),
    args: &WVAction,
) -> Result<(), WVError> {
    let bs = match args.block_size {
        Some(bs) => bs,
        None => {
            info!("Unknown block size, assuming 512");
            512
        }
    };
    let buf_size = ((bs * 2048) as usize).min(MAX_BUF_SIZE);
    let checkpoint_period = CHECKPOINT_BYTES / buf_size;

    let actual_input_bytes = WriteOp {
        file: &mut file,
        disk: &mut disk,
        cf: args.compression,
        buf_size,
        disk_block_size: bs as usize,
        checkpoint_period,
        file_read_buf_size: buf_size,
    }
    .execute(&mut tx)?;

    tx(WVEvent::FinishedWriting {
        verifying: args.verify,
    });

    if !args.verify {
        info!("Verification skip was requested, stopping");
        return Ok(());
    }

    info!("Rewinding source and target to beginning");
    file.seek(io::SeekFrom::Start(0))
        .map_err(IoError::<InputFileError>::from)?;
    disk.seek(io::SeekFrom::Start(0))
        .map_err(IoError::<DiskError>::from)?;

    if args.target_type == device::Type::File {
        info!(
            ?actual_input_bytes,
            "Output is a file, truncating to input length in case we wrote too much"
        );
        disk.0
            .set_len(actual_input_bytes)
            .map_err(IoError::<DiskError>::from)?;
    };

    info!("Executing verification");
    VerifyOp {
        file: &mut file,
        disk: &mut disk,
        cf: args.compression,
        buf_size,
        disk_block_size: bs as usize,
        checkpoint_period,
        file_read_buf_size: buf_size,
    }
    .execute(tx)?;

    Ok(())
}

/// Raw routine to execute `diskutil unmountdisk` on MacOS.
fn run_diskutil_umount(args: &WVAction) -> Result<(), CommandError> {
    let mut command = Command::new("diskutil");
    command
        .arg("unmountdisk")
        .arg(&args.dest)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    info!(?command, "spawning process to unmount disk");
    let mut child = command
        .spawn()
        .map_err(|e| CommandError::Process(e.into()))?;

    debug!("successfully ran diskutil, waiting on child process");
    let exit = child.wait().map_err(|e| CommandError::Process(e.into()))?;

    let mut stderr = String::new();
    child
        .stderr
        .take()
        .unwrap()
        .read_to_string(&mut stderr)
        .map_err(|e| CommandError::Comm(e.into()))?;
    let mut stdout = String::new();
    child
        .stdout
        .take()
        .unwrap()
        .read_to_string(&mut stdout)
        .map_err(|e| CommandError::Comm(e.into()))?;

    debug!(?exit, ?stderr, "child exited");

    if !exit.success() {
        return Err(CommandError::NonzeroExit {
            exit: exit.code(),
            stderr,
            stdout,
        });
    }

    Ok(())
}
