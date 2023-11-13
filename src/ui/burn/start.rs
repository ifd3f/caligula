use std::{fmt::Display, fs::File, path::PathBuf};

use bytesize::ByteSize;
use inquire::Confirm;
use tracing::debug;

use crate::{
    compression::CompressionFormat,
    device::WriteTarget,
    logging::get_log_paths,
    ui::{
        burn::{fancy::FancyUI, simple},
        cli::{BurnArgs, Interactive, UseSudo},
        utils::TUICapture,
    },
    writer_process::{
        self,
        handle::StartProcessError,
        ipc::{ErrorType, WriterProcessConfig},
    },
};

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct InputFileParams {
    pub file: PathBuf,
    pub size: ByteSize,
    pub compression: CompressionFormat,
}

impl InputFileParams {
    pub fn new(input_file: PathBuf, compression: CompressionFormat) -> std::io::Result<Self> {
        let input_file_size = ByteSize::b(File::open(&input_file)?.metadata()?.len());
        Ok(Self {
            file: input_file,
            size: input_file_size,
            compression,
        })
    }

    pub fn make_writer_config(&self, target: &WriteTarget) -> WriterProcessConfig {
        WriterProcessConfig {
            dest: target.devnode.clone(),
            src: self.file.clone(),
            logfile: get_log_paths().child.clone(),
            verify: true,
            compression: self.compression,
            target_type: target.target_type,
        }
    }
}

#[tracing::instrument(skip_all, fields(root, interactive))]
pub async fn try_start_burn(
    writer_config: &WriterProcessConfig,
    root: UseSudo,
    interactive: bool,
) -> anyhow::Result<writer_process::Handle> {
    let err = match writer_process::Handle::start(writer_config, false).await {
        Ok(p) => {
            return Ok(p);
        }
        Err(e) => e,
    };

    let dc = err.downcast::<StartProcessError>()?;

    if let StartProcessError::Failed(Some(ErrorType::PermissionDenied)) = &dc {
        match (root, interactive) {
            (UseSudo::Ask, true) => {
                debug!("Failure due to insufficient perms, asking user to escalate");

                let response = Confirm::new(&format!(
                    "We don't have permissions on {}. Escalate using sudo?",
                    writer_config.dest.to_string_lossy()
                ))
                .with_help_message(
                    "We will use the sudo command, which may prompt you for a password.",
                )
                .prompt()?;

                if response {
                    return writer_process::Handle::start(writer_config, true).await;
                }
            }
            (UseSudo::Always, _) => {
                return writer_process::Handle::start(writer_config, true).await;
            }
            _ => {}
        }
    }

    Err(dc.into())
}

pub async fn begin_writing(args: &BurnArgs, params: InputFileParams) -> anyhow::Result<()> {
    debug!("Opening TUI");
    if args.interactive.is_interactive() {
        debug!("Using fancy interactive TUI");
        let mut tui = TUICapture::new()?;
        let terminal = tui.terminal();

        // create app and run it
        FancyUI::new(&params, terminal).show().await?;
        debug!("Closing TUI");
    } else {
        debug!("Using simple TUI");
        simple::run_simple_ui(args, params).await?;
    }

    Ok(())
}
