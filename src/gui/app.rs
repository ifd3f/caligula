use egui::{CentralPanel, Color32, MenuBar, Panel, RichText, ViewportCommand};
use std::path::PathBuf;

use crate::{
    compression::CompressionFormat,
    hash::{HashAlg, parse_hash_input},
};

#[derive(Default)]
#[cfg_attr(debug_assertions, derive(serde::Deserialize, serde::Serialize))]
pub struct App {
    picked_image: Option<PathBuf>,
    file_hash_str: String,
    file_hash_algorithms_possible: Vec<HashAlg>,
    file_hash_algorithm_selected: Option<HashAlg>,
    latest_hashing_error: String,
    skip_hashing: bool,
}

impl App {
    #[cfg(not(debug_assertions))]
    pub fn new(_cc: &eframe::CreationContext) -> Self {
        Self::default()
    }

    #[cfg(debug_assertions)]
    pub fn new(cc: &eframe::CreationContext) -> Self {
        cc.storage
            .and_then(|storage| eframe::get_value(storage, eframe::APP_KEY))
            .unwrap_or_default()
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

            ui.strong("Image");
            if ui.button("💿 Pick file").clicked() {
                self.picked_image = rfd::FileDialog::new().pick_file();
            }
            if let Some(picked) = &self.picked_image {
                ui.label(picked.to_string_lossy());
                if let Some(cf) = CompressionFormat::detect_from_path(picked) {
                    ui.label(format!("Detected compression format: {}", cf));
                } else {
                    // ui.label(RichText::new("Couldn't detect compression format for picked image, assuming uncompressed!").color(Color32::YELLOW));
                }
            }

            ui.add_space(SECTION_SPACING * ui.spacing().item_spacing.y);

            ui.horizontal(|ui| {
                ui.strong("File hash");
                ui.checkbox(&mut self.skip_hashing, "Skip?");
            });

            ui.add_enabled_ui(!self.skip_hashing, |ui| {
                ui.label("We will guess the hash algorithm from your input.");
                if ui.text_edit_singleline(&mut self.file_hash_str).changed() {
                    match parse_hash_input(&self.file_hash_str) {
                        Ok((algs, _)) => {
                            self.file_hash_algorithms_possible = algs;
                            self.latest_hashing_error.clear();
                        }
                        Err(e) => {
                            self.file_hash_algorithms_possible = vec![];
                            self.file_hash_algorithm_selected = None;
                            self.latest_hashing_error = e.to_string();
                        }
                    }
                }

                if self.skip_hashing {
                    return;
                }

                if !self.latest_hashing_error.is_empty() {
                    ui.label(RichText::new(&self.latest_hashing_error).color(Color32::RED));
                }
                ui.horizontal(|ui| {
                    for alg in &self.file_hash_algorithms_possible {
                        let is_selected = Some(*alg) == self.file_hash_algorithm_selected;

                        if ui.selectable_label(is_selected, alg.to_string()).clicked() {
                            self.file_hash_algorithm_selected = Some(*alg);
                        }
                    }
                });
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
