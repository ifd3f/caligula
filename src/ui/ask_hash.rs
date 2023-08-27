use std::{
    fs::File,
    io::{BufReader, Seek},
    path::Path,
    process::exit,
};

use bytesize::ByteSize;
use indicatif::{ProgressBar, ProgressStyle};
use inquire::{Select, Text};

use crate::{
    compression::{decompress, CompressionFormat},
    hash::{parse_hash_input, FileHashInfo, HashAlg, Hashing},
};

use super::cli::{BurnArgs, HashArg, HashOf};

#[tracing::instrument(skip_all, fields(cf))]
pub fn ask_hash(args: &BurnArgs, cf: CompressionFormat) -> anyhow::Result<Option<FileHashInfo>> {
    let hash_params = match &args.hash {
        HashArg::Skip => None,
        HashArg::Ask => ask_hash_loop(cf)?,
        HashArg::Hash { alg, expected_hash } => Some(BeginHashParams {
            expected_hash: expected_hash.clone(),
            alg: alg.clone(),
            hasher_compression: ask_hasher_compression(cf, args.hash_of)?,
        }),
    };

    let params = if let Some(p) = hash_params {
        p
    } else {
        return Ok(None);
    };

    let hash_result = do_hashing(&args.input, &params)?;

    if hash_result.file_hash == params.expected_hash {
        eprintln!("Disk image verified successfully!");
    } else {
        eprintln!("Hash did not match!");
        eprintln!(
            "  Expected: {}",
            base16::encode_lower(&params.expected_hash)
        );
        eprintln!(
            "    Actual: {}",
            base16::encode_lower(&hash_result.file_hash)
        );
        eprintln!("Your disk image may be corrupted!");
        exit(-1);
    }

    Ok(Some(hash_result))
}

#[tracing::instrument]
fn ask_hash_loop(cf: CompressionFormat) -> anyhow::Result<Option<BeginHashParams>> {
    loop {
        match ask_hash_once(cf) {
            Ok(bhp) => {
                return Ok(Some(bhp));
            }
            Err(e) => match e.downcast::<Recoverable>()? {
                Recoverable::AskAgain => {
                    continue;
                }
                Recoverable::Skip => {
                    return Ok(None);
                }
            },
        }
    }
}

#[tracing::instrument]
fn ask_hash_once(cf: CompressionFormat) -> anyhow::Result<BeginHashParams> {
    let input_hash = Text::new("What is the file's hash?")
        .with_help_message(
            "We will guess the hash algorithm from your input. Press ESC or type \"skip\" to skip.",
        )
        .prompt_skippable()?;

    let (algs, hash) = match input_hash.as_deref() {
        None | Some("skip") => Err(Recoverable::Skip)?,
        Some(hash) => match parse_hash_input(hash) {
            Ok(hash) => hash,
            Err(e) => {
                eprintln!("{e}");
                Err(Recoverable::AskAgain)?
            }
        },
    };

    let alg = match &algs[..] {
        &[] => {
            eprintln!("Could not detect the hash algorithm from your hash!");
            Err(Recoverable::AskAgain)?
        }
        &[only_alg] => {
            eprintln!("Detected {}", only_alg);
            only_alg
        }
        multiple => {
            let ans = Select::new("Which algorithm is it?", multiple.into()).prompt_skippable()?;
            if let Some(alg) = ans {
                alg
            } else {
                Err(Recoverable::AskAgain)?
            }
        }
    };

    let hasher_compression = ask_hasher_compression(cf, None)?;

    Ok(BeginHashParams {
        expected_hash: hash,
        alg,
        hasher_compression,
    })
}

#[tracing::instrument]
fn ask_hasher_compression(
    cf: CompressionFormat,
    hash_of: Option<HashOf>,
) -> anyhow::Result<CompressionFormat> {
    if cf.is_identity() {
        return Ok(cf);
    }

    let ans = hash_of.map(Ok).unwrap_or_else(|| {
        Select::new(
            "Is the hash calculated from the raw file or the compressed file?",
            vec![HashOf::Raw, HashOf::Compressed],
        )
        .prompt()
    })?;

    Ok(match ans {
        HashOf::Raw => cf,
        HashOf::Compressed => CompressionFormat::Identity,
    })
}

#[tracing::instrument(skip_all, fields(path))]
fn do_hashing(path: &Path, params: &BeginHashParams) -> anyhow::Result<FileHashInfo> {
    let mut file = File::open(path)?;

    // Calculate total file size
    let file_size = file.seek(std::io::SeekFrom::End(0))?;
    file.seek(std::io::SeekFrom::Start(0))?;

    let progress_bar = ProgressBar::new(file_size);
    progress_bar.set_style(
        ProgressStyle::with_template("{bytes:>10} / {total_bytes:<10} {wide_bar}").unwrap(),
    );

    let decompress = decompress(params.hasher_compression, BufReader::new(file))?;

    let mut hashing = Hashing::new(
        params.alg,
        decompress,
        ByteSize::kib(512).as_u64() as usize, // TODO
    );
    loop {
        for _ in 0..32 {
            match hashing.next() {
                Some(_) => {}
                None => return Ok(hashing.finalize()?),
            }
        }
        progress_bar.set_position(hashing.get_reader_mut().get_mut().stream_position()?);
    }
}

#[derive(Debug)]
struct BeginHashParams {
    expected_hash: Vec<u8>,
    alg: HashAlg,
    hasher_compression: CompressionFormat,
}

/// A signaling error for the outer loop.
#[derive(Debug, thiserror::Error)]
#[error("Recoverable error")]
enum Recoverable {
    AskAgain,
    Skip,
}
