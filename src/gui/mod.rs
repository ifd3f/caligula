mod app;
mod sections;

use app::App;

pub fn run_gui() -> eframe::Result {
    eframe::run_native(
        "caligula-gui",
        Default::default(),
        Box::new(|cc| Ok(Box::new(App::new(cc)))),
    )
}
