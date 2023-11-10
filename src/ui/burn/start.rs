use std::{fmt::Display, fs::File, path::PathBuf};

use bytesize::ByteSize;
use inquire::Confirm;
use tracing::debug;

use crate::{
    writer_process::{
        self,
        handle::StartProcessError,
        ipc::{WriterProcessConfig, ErrorType},
    },
    compression::CompressionFormat,
    device::WriteTarget,
    logging::get_log_paths,
    ui::{
        burn::{fancy::FancyUI, simple},
        cli::{Interactive, UseSudo},
        utils::TUICapture,
    },
};

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct BeginParams {
    pub input_file: PathBuf,
    pub input_file_size: ByteSize,
    pub compression: CompressionFormat,
    pub target: WriteTarget,
}

impl BeginParams {
    pub fn new(
        input_file: PathBuf,
        compression: CompressionFormat,
        target: WriteTarget,
    ) -> std::io::Result<Self> {
        let input_file_size = ByteSize::b(File::open(&input_file)?.metadata()?.len());
        Ok(Self {
            input_file,
            input_file_size,
            compression,
            target,
        })
    }

    pub fn make_child_config(&self) -> WriterProcessConfig {
        WriterProcessConfig {
            dest: self.target.devnode.clone(),
            src: self.input_file.clone(),
            logfile: get_log_paths().child.clone(),
            verify: true,
            compression: self.compression,
            target_type: self.target.target_type,
        }
    }
}

#[tracing::instrument(skip_all, fields(root, interactive))]
pub async fn try_start_burn(
    args: &WriterProcessConfig,
    root: UseSudo,
    interactive: bool,
) -> anyhow::Result<writer_process::Handle> {
    let err = match writer_process::Handle::start(args, false).await {
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
                    args.dest.to_string_lossy()
                ))
                .with_help_message(
                    "We will use the sudo command, which may prompt you for a password.",
                )
                .prompt()?;

                if response {
                    return writer_process::Handle::start(args, true).await;
                }
            }
            (UseSudo::Always, _) => {
                return writer_process::Handle::start(args, true).await;
            }
            _ => {}
        }
    }

    Err(dc.into())
}

pub async fn begin_writing(
    interactive: Interactive,
    params: BeginParams,
    handle: writer_process::Handle,
) -> anyhow::Result<()> {
    debug!("Opening TUI");
    if interactive.is_interactive() {
        debug!("Using fancy interactive TUI");
        let mut tui = TUICapture::new()?;
        let terminal = tui.terminal();

        // create app and run it
        FancyUI::new(&params, handle, terminal).show().await?;
        debug!("Closing TUI");
    } else {
        debug!("Using simple TUI");
        simple::run_simple_ui(handle, params.compression).await?;
    }

    Ok(())
}

impl Display for BeginParams {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Input: {}", self.input_file.to_string_lossy())?;
        if self.compression.is_identity() {
            writeln!(f, "  Size: {}", self.input_file_size)?;
        } else {
            writeln!(f, "  Size (compressed): {}", self.input_file_size)?;
        }
        writeln!(f, "  Compression: {}", self.compression)?;
        writeln!(f)?;

        writeln!(f, "Output: {}", self.target.name)?;
        writeln!(f, "  Model: {}", self.target.model)?;
        writeln!(f, "  Size: {}", self.target.size)?;
        writeln!(f, "  Type: {}", self.target.target_type)?;
        writeln!(f, "  Path: {}", self.target.devnode.to_string_lossy())?;
        writeln!(f, "  Removable: {}", self.target.removable)?;

        Ok(())
    }
}
