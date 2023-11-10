use writer_process::child::is_in_writer_mode;

mod byteseries;
mod compression;
mod device;
mod escalation;
mod hash;
mod logging;
mod native;
mod ui;
mod writer_process;

fn main() {
    if is_in_writer_mode() {
        writer_process::child::main();
    } else {
        ui::main::main();
    }
}
