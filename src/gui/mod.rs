mod app;
mod sections;

use app::App;
use std::sync::Arc;

use crate::{facade::CaligulaFacade, logging::LogPaths, runtime::RemoteSpawn};

pub fn main(
    runtime: impl RemoteSpawn + Send + 'static,
    orc: Arc<impl CaligulaFacade>,
    log_paths: Arc<LogPaths>,
) -> eframe::Result<()> {
    eframe::run_native(
        "caligula-gui",
        Default::default(),
        Box::new(|cc| Ok(Box::new(App::new(cc, runtime, orc, log_paths)))),
    )
}
