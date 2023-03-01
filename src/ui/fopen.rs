use std::{
    fs::{File, OpenOptions},
    io,
    path::Path,
};

use inquire::{Confirm, InquireError};
use sudo::RunningAs;

pub fn open_or_escalate(path: impl AsRef<Path>) -> Result<File, Error> {
    match OpenOptions::new().write(true).open(&path) {
        Ok(file) => Ok(file),
        Err(e) => match e.kind() {
            io::ErrorKind::PermissionDenied => {
                let path = path.as_ref();
                let str_path = path.to_string_lossy();

                let running_as = sudo::check();
                if running_as == RunningAs::Root {
                    Err(Error::RootHasInsufficientPerms(
                        path.to_string_lossy().into_owned(),
                    ))?
                }

                let escalate = Confirm::new(
                    format!("We don't have write permissions on {str_path}. Escalate to root?")
                        .as_str(),
                )
                .with_default(true)
                .prompt()?;

                if !escalate {
                    Err(Error::UserDidNotEscalate)?
                }

                sudo::escalate_if_needed().map_err(|e| Error::EscalateFail(format!("{}", e)))?;
                open_or_escalate(path)
            }
            _ => todo!(),
        },
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Root does not have write permissions on file: {0}")]
    RootHasInsufficientPerms(String),
    #[error("User did not want to escalate")]
    UserDidNotEscalate,
    #[error("Failed to escalate into sudo: {0}")]
    EscalateFail(String),
    #[error("Failure during prompt: {0}")]
    PromptFail(#[from] InquireError),
}
