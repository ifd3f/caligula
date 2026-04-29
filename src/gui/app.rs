use std::path::PathBuf;

use egui::{CentralPanel, MenuBar, Panel, RichText, ViewportCommand};

#[derive(Default)]
pub struct App {
    picked_image: Option<PathBuf>,
}

impl App {
    pub fn new(_cc: &eframe::CreationContext) -> Self {
        Self::default()
    }
}

impl eframe::App for App {
    fn logic(&mut self, _ctx: &egui::Context, _frame: &mut eframe::Frame) {}

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        Panel::top("top_menu").show_inside(ui, |ui| {
            MenuBar::new().ui(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("❌ Quit").clicked() {
                        ui.ctx().send_viewport_cmd(ViewportCommand::Close);
                    }
                });
            });
        });

        CentralPanel::default().show_inside(ui, |ui| {
            ui.label(RichText::new(env!("CARGO_PKG_NAME")).heading().size(26.));
            ui.label(env!("CARGO_PKG_DESCRIPTION"));

            ui.add_space(4. * ui.spacing().item_spacing.y);

            ui.label("Input image to burn");
            ui.horizontal(|ui| {
                if ui.button("Pick file").clicked() {
                    self.picked_image = rfd::FileDialog::new().pick_file();
                }
                if let Some(picked) = &self.picked_image {
                    ui.label(picked.to_string_lossy());
                }
            });
        });
    }

    fn save(&mut self, _storage: &mut dyn eframe::Storage) {}
}
