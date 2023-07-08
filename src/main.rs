use burn::child::is_in_burn_mode;

mod burn;
mod byteseries;
mod compression;
mod device;
mod escalation;
mod hash;
mod logging;
mod native;
mod ui;

#[cfg(feature = "windows-media")]
mod windows_media;

fn main() {
    if is_in_burn_mode() {
        burn::child::main();
    } else {
        ui::main::main();
    }
}
