use egui::{
    CentralPanel, Checkbox, Color32, ComboBox, MenuBar, Panel, ProgressBar, RichText, UiBuilder,
    ViewportCommand,
};
use std::{
    path::PathBuf,
    sync::{
        Arc, Mutex,
        mpsc::{self, Receiver, Sender},
    },
    time::Duration,
};
use tracing::{error, info};

use crate::{
    compression::CompressionFormat,
    device::{self, Removable, WriteTarget, enumerate_devices},
    facade::{CaligulaFacade, WVState, WriteVerifyWorkflow, watch::Watch},
    hash::{HashAlg, parse_hash_input},
    logging::LogPaths,
    runtime::RemoteSpawn,
    ui::FacadeExt,
};

pub struct App {
    pub options: Options,
    pub main_to_worker_tx: Sender<WorkerEvent>,
    pub write_verify_state: Arc<Mutex<Option<Watch<WVState>>>>,
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

pub enum WorkerEvent {
    StartWrite(WriteVerifyWorkflow),
    Abort,
}

impl App {
    fn spawn_worker_thread(
        orc: Arc<impl CaligulaFacade>,
        runtime: impl RemoteSpawn + Send + 'static,
        ui_ctx: egui::Context,
        main_to_worker_rx: Receiver<WorkerEvent>,
        write_verify_state: Arc<Mutex<Option<Watch<WVState>>>>,
    ) {
        const REFRESH_PERIOD: Duration = Duration::from_millis(250);

        std::thread::spawn(move || {
            'outer: loop {
                let Ok(WorkerEvent::StartWrite(write_verify_workflow)) =
                    main_to_worker_rx.try_recv()
                else {
                    info!("nothing happening in worker thread, sleeping...");
                    std::thread::sleep(REFRESH_PERIOD);
                    continue;
                };

                let state = match orc
                    .clone()
                    .start_write_verify_blocking(&runtime, write_verify_workflow)
                {
                    Err(e) => {
                        error!(?e, "failed to start write/verify process");
                        continue;
                    }
                    Ok(state) => state,
                };

                write_verify_state.lock().unwrap().replace(state.clone());

                while !matches!(&*state.borrow(), WVState::Finished { .. }) {
                    ui_ctx.request_repaint();
                    std::thread::sleep(REFRESH_PERIOD);

                    if matches!(main_to_worker_rx.try_recv(), Ok(WorkerEvent::Abort)) {
                        write_verify_state.lock().unwrap().take();
                        ui_ctx.request_repaint();

                        // `state` goes out of scope here.
                        // TODO:
                        // find out if dropping `state` cancels the
                        // WriteVerifyWorkflow.
                        // or do we just lose control of it?
                        continue 'outer;
                    }
                }

                while !matches!(main_to_worker_rx.try_recv(), Ok(WorkerEvent::Abort)) {
                    std::thread::sleep(REFRESH_PERIOD);
                }

                write_verify_state.lock().unwrap().take();
                ui_ctx.request_repaint();
            }
        });
    }

    pub fn new(
        cc: &eframe::CreationContext,
        runtime: impl RemoteSpawn + Send + 'static,
        orc: Arc<impl CaligulaFacade>,
        log_paths: Arc<LogPaths>,
    ) -> Self {
        #[cfg(not(debug_assertions))]
        let options = Options::default();

        #[cfg(debug_assertions)]
        let options: Options = cc
            .storage
            .and_then(|storage| eframe::get_value(storage, eframe::APP_KEY))
            .unwrap_or_default();

        let (main_to_worker_tx, main_to_worker_rx) = mpsc::channel();

        let write_verify_state = Arc::new(Mutex::new(None));

        Self::spawn_worker_thread(
            orc,
            runtime,
            cc.egui_ctx.clone(),
            main_to_worker_rx,
            write_verify_state.clone(),
        );

        let mut s = Self {
            options,
            main_to_worker_tx,
            write_verify_state,
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
            if let Some(write_verify_state) = &*self.write_verify_state.lock().unwrap() {
                match &*write_verify_state.borrow() {
                    WVState::Writing(writing) => {
                        ui.label("Burning");
                        ui.add(
                            ProgressBar::new(writing.approximate_ratio() as f32).show_percentage(),
                        );
                        if ui.button("Abort").clicked() {
                            self.main_to_worker_tx.send(WorkerEvent::Abort).unwrap();
                        }
                    }
                    WVState::Verifying {
                        total_write_bytes,
                        verify_hist,
                        ..
                    } => {
                        ui.label("Verifying");
                        let ratio =
                            verify_hist.bytes_encountered() as f32 / *total_write_bytes as f32;
                        ui.add(ProgressBar::new(ratio).show_percentage());
                        if ui.button("Abort").clicked() {
                            self.main_to_worker_tx.send(WorkerEvent::Abort).unwrap();
                        }
                    }
                    WVState::Finished {
                        finish_time,
                        result,
                        total_write_bytes,
                        ..
                    } => {
                        ui.label(format!(
                            "Finished in {finish_time:?} with result {result:?}"
                        ));
                        ui.label(format!("Total bytes written: {total_write_bytes}"));
                        if ui.button("Finish").clicked() {
                            self.main_to_worker_tx.send(WorkerEvent::Abort).unwrap();
                        }
                    }
                }
                return;
            }

            ui.label(RichText::new(env!("CARGO_PKG_NAME")).heading().size(26.));
            ui.label(env!("CARGO_PKG_DESCRIPTION"));

            ui.add_space(SECTION_SPACING * ui.spacing().item_spacing.y);

            self.image_ui(ui);

            ui.add_space(SECTION_SPACING * ui.spacing().item_spacing.y);

            ui.add_enabled_ui(self.options.picked_image.is_some(), |ui| {
                self.file_hash_ui(ui)
            });

            ui.add_space(SECTION_SPACING * ui.spacing().item_spacing.y);

            ui.add_enabled_ui(self.file_hash_is_verified_or_skipped(), |ui| {
                self.target_disk_ui(ui)
            });

            ui.add_space(SECTION_SPACING * ui.spacing().item_spacing.y);

            ui.add_enabled_ui(self.options.selected_write_target.is_some(), |ui| {
                self.add_begin_writing_ui(ui)
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
