use egui::{Checkbox, Color32, ComboBox, RichText, UiBuilder};

use crate::{
    compression::CompressionFormat,
    device::{self},
    facade::WriteVerifyWorkflow,
    gui::app::{App, FileHashOptions, Options, WorkerEvent},
    hash::parse_hash_input,
};

impl App {
    pub fn file_hash_ui(&mut self, ui: &mut egui::Ui) {
        let FileHashOptions {
            entered_hash: file_hash_str,
            possible_algorithms: file_hash_algorithms_possible,
            selected_algorithm: file_hash_algorithm_selected,
            last_error: latest_hashing_error,
            skip: skip_hashing,
            verified: verified_hash,
        } = &mut self.options.file_hash;

        ui.horizontal(|ui| {
            ui.strong("File hash");
            ui.checkbox(skip_hashing, "Skip?");
        });

        ui.scope_builder(
            UiBuilder {
                disabled: *skip_hashing,
                // invisible: *skip_hashing,
                ..Default::default()
            },
            |ui| {
                ui.label("We will guess the hash algorithm from your input.");
                if ui.text_edit_singleline(file_hash_str).changed() {
                    match parse_hash_input(file_hash_str) {
                        Ok((algs, _)) => {
                            if algs.len() == 1 {
                                *file_hash_algorithm_selected = Some(algs[0]);
                            }
                            *file_hash_algorithms_possible = algs;
                            latest_hashing_error.clear();
                        }
                        Err(e) => {
                            *file_hash_algorithms_possible = vec![];
                            *file_hash_algorithm_selected = None;
                            *latest_hashing_error = e.to_string();
                        }
                    }
                }

                if latest_hashing_error.is_empty() {
                    ui.horizontal(|ui| {
                        for alg in file_hash_algorithms_possible {
                            let is_selected = Some(*alg) == *file_hash_algorithm_selected;

                            if ui.selectable_label(is_selected, alg.to_string()).clicked() {
                                *file_hash_algorithm_selected = Some(*alg);
                            }
                        }
                    });

                    ui.horizontal(|ui| {
                        ui.add_enabled(false, Checkbox::without_text(verified_hash));
                        if ui.button("Verify").clicked() {
                            // TODO:
                            *verified_hash = true;
                        }
                    });
                } else if *skip_hashing {
                    ui.label("");
                } else {
                    ui.label(RichText::new(&*latest_hashing_error).color(Color32::RED));
                }
            },
        );
    }

    pub fn image_ui(&mut self, ui: &mut egui::Ui) {
        let Options {
            detected_compression_format,
            picked_image,
            ..
        } = &mut self.options;

        ui.strong("Image");
        if ui.button("💿 Pick file").clicked()
            && let Some(picked) = rfd::FileDialog::new().pick_file()
        {
            *detected_compression_format = CompressionFormat::detect_from_path(&picked);
            *picked_image = Some(picked);
        }
        if let Some(picked) = picked_image {
            ui.label(picked.to_string_lossy());
            if let Some(cf) = detected_compression_format {
                ui.label(format!("Detected compression format: {}", cf));
            } else {
                ui.label(
                RichText::new(
                    "Couldn't detect compression format for picked image, assuming uncompressed!",
                )
                .color(Color32::YELLOW),
            );
            }
        }
    }

    pub fn target_disk_ui(&mut self, ui: &mut egui::Ui) {
        ui.strong("Target disk");
        if ui.button("Refresh devices").clicked() {
            self.refresh_devices();
        }

        // FIXME:
        // - stop alloc:ing and doing so much work here.. DON'T CLONE!
        // - move the label formatting into a place where it's done ONCE, not on every ui render!
        // - deduplicate, label formatting is stolen from `ask_outfile.rs`
        ComboBox::from_label(format!(
            "{} available",
            self.options.possible_write_targets.len()
        ))
        .selected_text(
            self.options
                .selected_write_target
                .as_ref()
                .map(|dev| match dev.target_type {
                    device::Type::Disk => format!(
                        "{} | {} - {} ({}, removable: {})",
                        dev.name, dev.model, dev.size, dev.target_type, dev.removable
                    ),
                    _ => format!(
                        "{} | {} - {} ({})",
                        dev.name, dev.model, dev.size, dev.target_type
                    ),
                })
                .unwrap_or_default(),
        )
        .show_ui(ui, |ui| {
            for dev in &self.options.possible_write_targets {
                let label = match dev.target_type {
                    device::Type::Disk => format!(
                        "{} | {} - {} ({}, removable: {})",
                        dev.name, dev.model, dev.size, dev.target_type, dev.removable
                    ),
                    _ => format!(
                        "{} | {} - {} ({})",
                        dev.name, dev.model, dev.size, dev.target_type
                    ),
                };
                ui.selectable_value(
                    &mut self.options.selected_write_target,
                    Some(dev.clone()),
                    label,
                );
            }
        });
    }

    pub fn add_begin_writing_ui(&mut self, ui: &mut egui::Ui) {
        ui.add_enabled_ui(self.is_ready_for_writing(), |ui| {
            ui.strong("Write");
            if ui.button("Prepare for writing").clicked() {
                self.options.has_confirmed_writing = true;
            }
            if self.options.has_confirmed_writing && ui.button("Write!").clicked() {
                // FIXME: unwrapping! if any are missing we need to show this to user instead
                // TODO: remove this button and automaatically populate this ingoing values all are Some
                let write_verify_workflow = WriteVerifyWorkflow::new(
                    self.options.picked_image.clone().unwrap(),
                    self.options.detected_compression_format.unwrap(),
                    self.options.selected_write_target.clone().unwrap(),
                )
                .unwrap();

                self.main_to_worker_tx
                    .send(WorkerEvent::StartWrite(write_verify_workflow))
                    .unwrap();

                self.options.has_confirmed_writing = false;
            }
        });
    }
}
