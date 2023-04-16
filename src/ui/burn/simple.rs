use std::time::Instant;

use indicatif::{ProgressBar, ProgressStyle};

use crate::{
    burn::{state_tracking::ChildState, Handle},
    compression::CompressionFormat,
};

pub async fn run_simple_ui(mut handle: Handle, cf: CompressionFormat) -> anyhow::Result<()> {
    let input_file_bytes = handle.initial_info().input_file_bytes;
    let write_progress = ProgressBar::new(100).with_message("Burning").with_style(
        ProgressStyle::with_template(
            "[{elapsed_precise}] {msg:>10} {wide_bar:.green/black} {percent:>3}%",
        )
        .unwrap(),
    );
    let verify_progress = ProgressBar::new(100).with_message("Verifying").with_style(
        ProgressStyle::with_template(
            "[{elapsed_precise}] {msg:>10} {wide_bar:.blue/black} {percent:>3}%",
        )
        .unwrap(),
    );

    let mut child_state = ChildState::initial(Instant::now(), !cf.is_identity(), input_file_bytes);

    loop {
        let x = handle.next_message().await?;
        child_state = child_state.on_status(Instant::now(), x);
        match &child_state {
            ChildState::Burning(b) => {
                write_progress.set_position((b.approximate_ratio() * 1000.0) as u64)
            }
            ChildState::Verifying {
                total_write_bytes, ..
            } => verify_progress.set_position(total_write_bytes * 1000 / input_file_bytes),
            ChildState::Finished { .. } => break,
        }
    }
    println!("Done!");
    Ok(())
}
