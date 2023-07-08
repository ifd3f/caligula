use std::path::Path;

use bytesize::ByteSize;
use gpt::GptConfig;

use crate::ui::cli::WinMediaArgs;

pub async fn make_windows_media(args: WinMediaArgs) -> anyhow::Result<()> {
    todo!()
}

pub fn write(device: impl AsRef<Path>) -> anyhow::Result<()> {
    let mut gpt = GptConfig::new()
        .writable(true)
        .initialized(false)
        .open(device)?;

    let part = gpt.add_partition(
        "",
        ByteSize::mb(550).as_u64(),
        gpt::partition_types::EFI,
        0,
        None,
    )?;

    Ok(())
}
