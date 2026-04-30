use egui::{CentralPanel, MenuBar, Panel, RichText, ViewportCommand};
use std::path::PathBuf;

use crate::{
    compression::CompressionFormat,
    device::{Removable, WriteTarget, enumerate_devices},
    gui::sections::{add_begin_writing_ui, add_file_hash_ui, add_image_ui, add_target_disk_ui},
    hash::HashAlg,
};

#[derive(Default)]
#[cfg_attr(debug_assertions, derive(serde::Deserialize, serde::Serialize))]
pub struct App {
    pub picked_image: Option<PathBuf>,
    pub file_hash_options: FileHashOptions,
    pub detected_compression_format: Option<CompressionFormat>,
    #[cfg_attr(debug_assertions, serde(skip))]
    pub possible_write_targets: Vec<WriteTarget>,
    #[cfg_attr(debug_assertions, serde(skip))]
    pub selected_write_target: Option<WriteTarget>,
    pub show_all_disks: bool,
}

#[derive(Default)]
#[cfg_attr(debug_assertions, derive(serde::Deserialize, serde::Serialize))]
pub struct FileHashOptions {
    pub entered_hash: String,
    pub possible_algorithms: Vec<HashAlg>,
    pub selected_algorithm: Option<HashAlg>,
    pub last_error: String,
    pub skip: bool,
    #[cfg_attr(debug_assertions, serde(skip))]
    pub verified: bool,
}

impl App {
    #[cfg(not(debug_assertions))]
    pub fn new(_cc: &eframe::CreationContext) -> Self {
        let mut s = Self::default();
        s.refresh_devices();
        s
    }

    #[cfg(debug_assertions)]
    pub fn new(cc: &eframe::CreationContext) -> Self {
        let mut s: Self = cc
            .storage
            .and_then(|storage| eframe::get_value(storage, eframe::APP_KEY))
            .unwrap_or_default();
        s.refresh_devices();
        s
    }

    pub fn refresh_devices(&mut self) {
        // TODO: deduplicate this.
        // This is code stolen from `ask_outfile.rs`!
        self.possible_write_targets = enumerate_devices()
            .filter(|d| self.show_all_disks || d.removable == Removable::Yes)
            .collect();
        self.possible_write_targets.sort();
    }
}

impl eframe::App for App {
    fn logic(&mut self, _ctx: &egui::Context, _frame: &mut eframe::Frame) {}

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        const SECTION_SPACING: f32 = 6.;

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

            ui.add_space(SECTION_SPACING * ui.spacing().item_spacing.y);

            add_image_ui(self, ui);

            ui.add_space(SECTION_SPACING * ui.spacing().item_spacing.y);

            ui.add_enabled_ui(self.picked_image.is_some(), |ui| {
                add_file_hash_ui(&mut self.file_hash_options, ui)
            });

            ui.add_space(SECTION_SPACING * ui.spacing().item_spacing.y);

            ui.add_enabled_ui(
                self.file_hash_options.verified || self.file_hash_options.skip,
                |ui| add_target_disk_ui(self, ui),
            );

            ui.add_space(SECTION_SPACING * ui.spacing().item_spacing.y);

            ui.add_enabled_ui(self.selected_write_target.is_some(), |ui| {
                add_begin_writing_ui(self, ui)
            });
        });
    }

    #[cfg(not(debug_assertions))]
    fn save(&mut self, _storage: &mut dyn eframe::Storage) {}

    #[cfg(debug_assertions)]
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, eframe::APP_KEY, self);
    }
}
