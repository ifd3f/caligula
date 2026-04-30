mod app;
mod sections;

use app::App;
use std::sync::Arc;
use tokio::runtime::Handle;

use crate::logging::LogPaths;

pub fn main(log_paths: Arc<LogPaths>) -> eframe::Result<()> {
    eframe::run_native(
        "caligula-gui",
        Default::default(),
        Box::new(|cc| Ok(Box::new(App::new(cc, log_paths)))),
    )
}
