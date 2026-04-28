use egui::{Panel, ViewportCommand};

pub fn add_top_menu_bar(ui: &mut egui::Ui) {
    Panel::top("top_menu").show_inside(ui, |ui| {
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("File", |ui| {
                if ui.button("❌ Quit").clicked() {
                    ui.ctx().send_viewport_cmd(ViewportCommand::Close);
                }
            });
            ui.menu_button("Theme", |ui| {
                if ui.button("Dark").clicked() {
                    // TODO:
                }
                if ui.button("Light").clicked() {
                    // TODO:
                }
                if ui.button("System").clicked() {
                    // TODO:
                }
            });
        });
    });
}
