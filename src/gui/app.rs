use crate::gui::panels::add_top_menu_bar;

pub struct App {}

impl App {
    pub fn new(_cc: &eframe::CreationContext) -> Self {
        Self {}
    }
}

impl eframe::App for App {
    fn logic(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        let _ = (ctx, frame);
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        add_top_menu_bar(ui);
    }

    fn save(&mut self, _storage: &mut dyn eframe::Storage) {}
}
