use egui::{CentralPanel, MenuBar, Panel, RichText, ViewportCommand};
use std::{
    path::PathBuf,
    rc::Rc,
    sync::{Arc, Mutex},
};

use crate::{
    compression::CompressionFormat,
    device::{Removable, WriteTarget, enumerate_devices},
    gui::sections::{add_begin_writing_ui, add_file_hash_ui, add_image_ui, add_target_disk_ui},
    hash::HashAlg,
    logging::LogPaths,
    ui::BeginParams,
};

pub struct App {
    pub log_paths: Arc<LogPaths>,
    pub options: Options,
    pub ongoing_write: Rc<Mutex<Option<OngoingWrite>>>,
}

pub struct OngoingWrite {
    pub write_progress: u64,
    pub verify_progress: u64,
}

#[derive(Default)]
#[cfg_attr(debug_assertions, derive(serde::Deserialize, serde::Serialize))]
pub struct Options {
    pub picked_image: Option<PathBuf>,
    pub file_hash: FileHashOptions,
    pub detected_compression_format: Option<CompressionFormat>,
    #[cfg_attr(debug_assertions, serde(skip))]
    pub possible_write_targets: Vec<WriteTarget>,
    #[cfg_attr(debug_assertions, serde(skip))]
    pub selected_write_target: Option<WriteTarget>,
    pub show_all_disks: bool,
    #[cfg_attr(debug_assertions, serde(skip))]
    pub begin_params: Option<BeginParams>,
    #[cfg_attr(debug_assertions, serde(skip))]
    pub has_confirmed_writing: bool,
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
    pub fn new(cc: &eframe::CreationContext, log_paths: Arc<LogPaths>) -> Self {
        #[cfg(not(debug_assertions))]
        let options = Options::default();
        #[cfg(debug_assertions)]
        let options: Options = cc
            .storage
            .and_then(|storage| eframe::get_value(storage, eframe::APP_KEY))
            .unwrap_or_default();
        let mut s = Self {
            log_paths,
            options,
            ongoing_write: Rc::new(Mutex::new(None)),
        };
        s.refresh_devices();
        s
    }

    pub fn refresh_devices(&mut self) {
        // TODO: deduplicate this.
        // This is code stolen from `ask_outfile.rs`!
        self.options.possible_write_targets = enumerate_devices()
            .filter(|d| self.options.show_all_disks || d.removable == Removable::Yes)
            .collect();
        self.options.possible_write_targets.sort();
    }

    pub fn file_hash_is_verified_or_skipped(&self) -> bool {
        self.options.file_hash.verified || self.options.file_hash.skip
    }

    pub fn is_ready_for_writing(&self) -> bool {
        self.options.picked_image.is_some()
            && self.file_hash_is_verified_or_skipped()
            && self.options.selected_write_target.is_some()
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
            if let Some(ongoing_write) = &*self.ongoing_write.lock().unwrap() {
                ui.label("writing!!");
                return;
            }

            ui.label(RichText::new(env!("CARGO_PKG_NAME")).heading().size(26.));
            ui.label(env!("CARGO_PKG_DESCRIPTION"));

            ui.add_space(SECTION_SPACING * ui.spacing().item_spacing.y);

            add_image_ui(&mut self.options, ui);

            ui.add_space(SECTION_SPACING * ui.spacing().item_spacing.y);

            ui.add_enabled_ui(self.options.picked_image.is_some(), |ui| {
                add_file_hash_ui(&mut self.options.file_hash, ui)
            });

            ui.add_space(SECTION_SPACING * ui.spacing().item_spacing.y);

            ui.add_enabled_ui(self.file_hash_is_verified_or_skipped(), |ui| {
                add_target_disk_ui(self, ui)
            });

            ui.add_space(SECTION_SPACING * ui.spacing().item_spacing.y);

            ui.add_enabled_ui(self.options.selected_write_target.is_some(), |ui| {
                add_begin_writing_ui(self, ui)
            });
        });
    }

    #[cfg(not(debug_assertions))]
    fn save(&mut self, _storage: &mut dyn eframe::Storage) {}

    #[cfg(debug_assertions)]
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, eframe::APP_KEY, &self.options);
    }
}
