use egui::{Checkbox, Color32, ComboBox, RichText, UiBuilder};
use futures::StreamExt;
use std::time::Instant;
use tokio::task::LocalSet;

use crate::{
    compression::CompressionFormat,
    device,
    gui::app::{App, FileHashOptions, OngoingWrite, Options},
    hash::parse_hash_input,
    herder_facade::make_herder_facade_impl,
    ui::{BeginParams, Interactive, UseSudo, WriterState, try_start_burn},
};

pub fn add_file_hash_ui(hash_options: &mut FileHashOptions, ui: &mut egui::Ui) {
    let FileHashOptions {
        entered_hash: file_hash_str,
        possible_algorithms: file_hash_algorithms_possible,
        selected_algorithm: file_hash_algorithm_selected,
        last_error: latest_hashing_error,
        skip: skip_hashing,
        verified: verified_hash,
    } = hash_options;

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

pub fn add_image_ui(options: &mut Options, ui: &mut egui::Ui) {
    let Options {
        detected_compression_format,
        picked_image,
        ..
    } = options;

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

pub fn add_target_disk_ui(app: &mut App, ui: &mut egui::Ui) {
    ui.strong("Target disk");
    if ui.button("Refresh devices").clicked() {
        app.refresh_devices();
    }

    // FIXME:
    // - stop alloc:ing and doing so much work here.. DON'T CLONE!
    // - move the label formatting into a place where it's done ONCE, not on every ui render!
    // - deduplicate, label formatting is stolen from `ask_outfile.rs`
    ComboBox::from_label(format!(
        "{} available",
        app.options.possible_write_targets.len()
    ))
    .selected_text(
        app.options
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
        for dev in &app.options.possible_write_targets {
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
                &mut app.options.selected_write_target,
                Some(dev.clone()),
                label,
            );
        }
    });
}

pub fn add_begin_writing_ui(app: &mut App, ui: &mut egui::Ui) {
    ui.add_enabled_ui(app.is_ready_for_writing(), |ui| {
        ui.strong("Write");
        if ui.button("Prepare for writing").clicked() {
            // FIXME:
            // don't unwrap.
            // actually don't even have this shitty refresh button,
            // should just refresh when any of the underlying values change
            app.options.begin_params = BeginParams::new(
                app.options.picked_image.clone().unwrap(),
                app.options.detected_compression_format.unwrap(),
                app.options.selected_write_target.clone().unwrap(),
            )
            .ok();
        }

        if let Some(begin_params) = &app.options.begin_params {
            ui.label(begin_params.to_string());
            ui.label(RichText::new("Ready to write!").color(Color32::GREEN));

            if !app.options.has_confirmed_writing {
                if ui.button("Perform write").clicked() {
                    app.options.has_confirmed_writing = true;
                }
                return;
            }

            ui.label(
                RichText::new("THIS ACTION WILL DESTROY ALL DATA ON THIS DEVICE!!!")
                    .color(Color32::YELLOW),
            );

            if ui.button("I know, do it!").clicked() {
                // TODO: make sure this really needs to clone
                let log_paths = app.log_paths.clone();
                let begin_params = begin_params.clone();
                let cf = app.options.detected_compression_format.unwrap(); // FIXME:
                let ongoing_write = app.ongoing_write.clone();

                tokio::task::spawn_local(async move {
                    eprintln!("inside task!");
                    let mut herder = make_herder_facade_impl(log_paths.main());

                    // FIXME: parameters to `try_start_burn`
                    let interactive = Interactive::Never;
                    let mut handle = try_start_burn(
                        &mut herder,
                        &begin_params.make_child_config(),
                        UseSudo::Never,
                        interactive.is_interactive(),
                    )
                    .await?;

                    let input_file_bytes = handle.initial_info.input_file_bytes;

                    let mut child_state =
                        WriterState::initial(Instant::now(), !cf.is_identity(), input_file_bytes);

                    *ongoing_write.lock().unwrap() = Some(OngoingWrite {
                        write_progress: 0,
                        verify_progress: 0,
                    });

                    loop {
                        eprintln!("got event!");
                        let x = handle.events.next().await;
                        child_state = child_state.on_status(Instant::now(), x);
                        // FIXME: fugly-ass unwrapping
                        match &child_state {
                            WriterState::Writing(b) => {
                                ongoing_write
                                    .lock()
                                    .unwrap()
                                    .as_mut()
                                    .unwrap()
                                    .write_progress = (b.approximate_ratio() * 1000.0) as u64
                            }
                            WriterState::Verifying {
                                total_write_bytes, ..
                            } => {
                                ongoing_write
                                    .lock()
                                    .unwrap()
                                    .as_mut()
                                    .unwrap()
                                    .verify_progress = total_write_bytes * 1000 / input_file_bytes
                            }
                            WriterState::Finished { .. } => break,
                        }
                    }

                    anyhow::Ok(())
                });
            }
        }
    });
}
