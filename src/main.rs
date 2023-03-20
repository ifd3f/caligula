use burn::child::is_in_burn_mode;

mod burn;
mod byteseries;
mod compression;
mod device;
mod hash;
mod logging;
mod native;
mod ui;

fn main() {
    if is_in_burn_mode() {
        burn::child::main();
    } else {
        ui::main::main();
    }
}
