use std::{env, path::PathBuf, process, time::SystemTime};

use tokio::fs::DirBuilder;

/// Create the directory to shove invocation-specific data into, like log files and sockets.
pub async fn ensure_state_dir() -> Result<PathBuf, futures_io::Error> {
    let dir = env::temp_dir().join(format!(
        "caligula-{}-{}",
        process::id(),
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_millis(),
    ));

    DirBuilder::new()
        .mode(0o700)
        .recursive(true)
        .create(&dir)
        .await?;

    Ok(dir)
}
