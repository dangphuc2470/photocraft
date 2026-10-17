//! Contextual Task Bar and Generative Fill integration for PhotoCraft.
//! Displays an interactive floating taskbar under active selections to trigger
//! Gemini inpainting and Generative Fill on a new layer.

use egui::{Color32, CornerRadius, Margin, Rect, RichText, Stroke, pos2, vec2};
use photocraft_geom::Rect as DRect;

use crate::PhotocraftApp;

/// Calculates screen rectangle for the Contextual Task Bar
pub fn taskbar_rect(app: &PhotocraftApp, clip: Rect, screen_rect: Rect) -> Rect {
    let is_active = app.ui.gen_fill_active;
    let is_loading = app.ui.gen_fill_loading;
    let bar_width = if is_loading {
        440.0
    } else if is_active {
        440.0
    } else {
        230.0
    };
    let bar_height = 40.0;

    let pos_x = (screen_rect.center().x - bar_width / 2.0).clamp(clip.min.x + 12.0, clip.max.x - bar_width - 12.0);
    let mut pos_y = screen_rect.max.y + 14.0;
    if pos_y + bar_height > clip.max.y - 28.0 {
        // Position above selection if hugging the bottom canvas boundary
        pos_y = (screen_rect.min.y - bar_height - 14.0).max(clip.min.y + 36.0);
    }

    Rect::from_min_size(pos2(pos_x, pos_y), vec2(bar_width, bar_height))
}

/// Checks asynchronous inpainting results and renders the Contextual Taskbar under selection.
pub fn contextual_taskbar(
    app: &mut PhotocraftApp,
    ui: &mut egui::Ui,
    idx: usize,
    sel_rect: DRect,
    screen_rect: Rect,
) {
    // 1. Process background worker completion
    let mut finished_res = None;
    if let Some(rx) = &app.gen_fill_rx {
        if let Ok(res) = rx.try_recv() {
            finished_res = Some(res);
        }
    }

    if let Some((_doc_idx, target_sel, prompt, res)) = finished_res {
        app.ui.gen_fill_loading = false;
        app.gen_fill_rx = None;
        let _ = std::fs::remove_file(std::env::temp_dir().join("photocraft_genfill_status.txt"));
        match res {
            Ok(out_path) => {
                apply_generated_result(app, target_sel, &prompt, &out_path);
                app.ui.gen_fill_active = false;
                app.ui.gen_fill_prompt.clear();
                app.ui.gen_fill_status = None;
                // Deselect selection upon successful generation
                let _ = app.run("select.deselect", serde_json::json!({}));
            }
            Err(err) => {
                app.ui.gen_fill_status = Some(err);
            }
        }
    }

    // 2. Position the Contextual Task Bar
    let clip = ui.clip_rect();
    let is_loading = app.ui.gen_fill_loading;
    let is_active = app.ui.gen_fill_active;
    let bar_rect = taskbar_rect(app, clip, screen_rect);

    // 3. Render Floating Task Bar in Foreground Area
    let mut trigger_gen = false;
    let mut cancel_active = false;
    let mut deselect = false;
    let mut enter_active = false;

    let area_res = egui::Area::new(egui::Id::new("pc-contextual-taskbar"))
        .order(egui::Order::Foreground)
        .fixed_pos(bar_rect.min)
        .show(ui.ctx(), |ui| {
            egui::Frame::new()
                .fill(Color32::from_rgb(26, 26, 29))
                .stroke(Stroke::new(1.0, Color32::from_rgb(52, 53, 58)))
                .corner_radius(CornerRadius::same(9))
                .inner_margin(Margin::symmetric(10, 6))
                .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing = vec2(8.0, 0.0);

                    if is_loading {
                        ui.spinner();
                        let status_path = std::env::temp_dir().join("photocraft_genfill_status.txt");
                        let current_step = if let Ok(s) = std::fs::read_to_string(&status_path) {
                            let t = s.trim().to_string();
                            if !t.is_empty() {
                                t
                            } else {
                                "Processing inpaint with Gemini...".to_string()
                            }
                        } else {
                            "Processing inpaint with Gemini...".to_string()
                        };

                        let _ = ui.label(
                            RichText::new(&current_step)
                                .color(Color32::from_rgb(130, 200, 255))
                                .strong(),
                        )
                        .on_hover_text(format!(
                            "Generative Fill Progress:\n{}\n\nPrompt: {}\n(Detailed log is written to photocraft_genfill.log)",
                            current_step, app.ui.gen_fill_prompt
                        ));
                        ui.ctx().request_repaint_after(std::time::Duration::from_millis(200));
                    } else if !is_active {
                        let fill_btn = ui.button(
                            RichText::new("Generative Fill")
                                .color(Color32::WHITE)
                                .size(13.0)
                                .strong(),
                        )
                        .on_hover_text("Inpaint or modify active selection using generative AI (Gemini)");
                        if fill_btn.clicked() {
                            enter_active = true;
                            let edit_id = ui.make_persistent_id("gen_fill_prompt_edit");
                            let focus_key = egui::Id::new("gen_fill_prompt_focus");
                            ui.data_mut(|d| d.insert_temp(focus_key, true));
                            ui.memory_mut(|m| m.request_focus(edit_id));
                        }

                        ui.separator();

                        if ui.button("Deselect").clicked() {
                            deselect = true;
                        }
                    } else {
                        ui.label(RichText::new("AI").color(Color32::from_rgb(130, 195, 255)).strong().size(13.0));

                        let edit_id = ui.make_persistent_id("gen_fill_prompt_edit");
                        let text_edit = ui.add(
                            egui::TextEdit::singleline(&mut app.ui.gen_fill_prompt)
                                .id(edit_id)
                                .hint_text("Describe what you want to generate or modify...")
                                .desired_width(260.0),
                        );

                        let focus_key = egui::Id::new("gen_fill_prompt_focus");
                        if ui.data_mut(|d| d.remove_temp::<bool>(focus_key).unwrap_or(false)) {
                            text_edit.request_focus();
                        }

                        if text_edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                            trigger_gen = true;
                        }

                        let gen_btn = ui.add(
                            egui::Button::new(
                                RichText::new("Generate")
                                .color(Color32::WHITE)
                                .strong(),
                            )
                            .fill(Color32::from_rgb(26, 115, 232)),
                        );

                        if gen_btn.clicked() {
                            trigger_gen = true;
                        }

                        if ui.button("✕").clicked() {
                            cancel_active = true;
                        }
                    }

                    if let Some(status) = &app.ui.gen_fill_status {
                        ui.label(RichText::new(status).color(Color32::from_rgb(255, 100, 100)).size(11.0));
                    }
                });
            });
    });

    app.gen_fill_rect = Some(area_res.response.rect);

    if enter_active {
        app.ui.gen_fill_active = true;
    }
    if cancel_active {
        app.ui.gen_fill_active = false;
        app.ui.gen_fill_status = None;
    }
    if deselect {
        let _ = app.run("select.deselect", serde_json::json!({}));
    }
    if trigger_gen && !app.ui.gen_fill_prompt.trim().is_empty() {
        trigger_generation(app, &ui.ctx().clone(), idx, sel_rect);
    }
}

/// Spawns background worker thread to execute Gemini Inpainting
fn trigger_generation(
    app: &mut PhotocraftApp,
    ctx: &egui::Context,
    idx: usize,
    sel_rect: DRect,
) {
    let docs = app.session.documents();
    if idx >= docs.len() {
        return;
    }
    let doc = &docs[idx].doc;

    // 1. Render cropped selection region directly from document
    let buf = photocraft_compose::render(doc, sel_rect);
    let rgba8 = buf.to_rgba8();

    let img = match photocraft_codecs::Image::from_raw(
        rgba8.width,
        rgba8.height,
        photocraft_codecs::ChannelLayout::Rgba,
        photocraft_codecs::SampleType::U8,
        rgba8.pixels,
    ) {
        Ok(i) => i,
        Err(e) => {
            app.ui.gen_fill_status = Some(format!("Image error: {e}"));
            return;
        }
    };

    let png_bytes = match photocraft_codecs::encode(
        &img,
        photocraft_codecs::Format::Png,
        &photocraft_codecs::EncodeOptions::default(),
    ) {
        Ok(b) => b,
        Err(e) => {
            app.ui.gen_fill_status = Some(format!("Encode error: {e}"));
            return;
        }
    };

    // 2. Prepare paths
    let temp_dir = std::env::temp_dir();
    let in_path = temp_dir.join("photocraft_genfill_in.png");
    let out_path = temp_dir.join("photocraft_genfill_out.png");
    let log_path = temp_dir.join("photocraft_genfill.log");
    let status_path = temp_dir.join("photocraft_genfill_status.txt");

    if let Err(e) = std::fs::write(&in_path, png_bytes) {
        app.ui.gen_fill_status = Some(format!("Disk write error: {e}"));
        return;
    }

    let _ = std::fs::write(&status_path, "[1/5] Cropping and preparing selection...");

    if out_path.exists() {
        let _ = std::fs::remove_file(&out_path);
    }

    // 3. Launch async generation worker
    let (tx, rx) = std::sync::mpsc::channel();
    app.gen_fill_rx = Some(rx);
    app.ui.gen_fill_loading = true;
    app.ui.gen_fill_status = None;

    let prompt = app.ui.gen_fill_prompt.clone();
    let ctx_clone = ctx.clone();

    std::thread::spawn(move || {
        let node_exe = "C:\\Program Files\\nodejs\\node.exe";
        let app_data = std::env::var("APPDATA").unwrap_or_else(|_| "C:\\Users\\phucdnh\\AppData\\Roaming".into());
        let worker_js = format!("{}\\9router\\photoshop\\worker.js", app_data);

        let in_str = in_path.to_string_lossy().to_string();
        let out_str = out_path.to_string_lossy().to_string();
        let log_str = log_path.to_string_lossy().to_string();
        let status_str = status_path.to_string_lossy().to_string();

        #[cfg(target_os = "windows")]
        use std::os::windows::process::CommandExt;
        #[cfg(target_os = "windows")]
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;

        let mut cmd = std::process::Command::new(node_exe);
        #[cfg(target_os = "windows")]
        cmd.creation_flags(CREATE_NO_WINDOW);

        let status = cmd
            .arg(&worker_js)
            .arg("--prompt")
            .arg(&prompt)
            .arg("--image")
            .arg(&in_str)
            .arg("--output")
            .arg(&out_str)
            .arg("--log")
            .arg(&log_str)
            .arg("--status")
            .arg(&status_str)
            .status();

        let result = match status {
            Ok(s) if s.success() && out_path.exists() => Ok(out_str),
            Ok(s) => Err(format!("Worker failed with exit code {:?}", s.code())),
            Err(e) => Err(format!("Failed to spawn worker process: {e}")),
        };

        let _ = tx.send((idx, sel_rect, prompt, result));
        ctx_clone.request_repaint();
    });
}

/// Resamples an RGBA8 image buffer from (src_w, src_h) to (dst_w, dst_h) using bilinear interpolation
fn rescale_rgba8(src: &[u8], src_w: u32, src_h: u32, dst_w: u32, dst_h: u32) -> Vec<u8> {
    if src_w == dst_w && src_h == dst_h {
        return src.to_vec();
    }
    if dst_w == 0 || dst_h == 0 || src_w == 0 || src_h == 0 {
        return Vec::new();
    }

    let mut dst = vec![0u8; (dst_w * dst_h * 4) as usize];
    let x_scale = src_w as f32 / dst_w as f32;
    let y_scale = src_h as f32 / dst_h as f32;

    for y in 0..dst_h {
        let src_y = (y as f32 + 0.5) * y_scale - 0.5;
        let y0 = (src_y.floor() as i32).clamp(0, src_h as i32 - 1) as u32;
        let y1 = (y0 + 1).min(src_h - 1);
        let dy = (src_y - y0 as f32).clamp(0.0, 1.0);

        for x in 0..dst_w {
            let src_x = (x as f32 + 0.5) * x_scale - 0.5;
            let x0 = (src_x.floor() as i32).clamp(0, src_w as i32 - 1) as u32;
            let x1 = (x0 + 1).min(src_w - 1);
            let dx = (src_x - x0 as f32).clamp(0.0, 1.0);

            let dst_idx = ((y * dst_w + x) * 4) as usize;

            let p00 = ((y0 * src_w + x0) * 4) as usize;
            let p10 = ((y0 * src_w + x1) * 4) as usize;
            let p01 = ((y1 * src_w + x0) * 4) as usize;
            let p11 = ((y1 * src_w + x1) * 4) as usize;

            for c in 0..4 {
                let v00 = src.get(p00 + c).copied().unwrap_or(0) as f32;
                let v10 = src.get(p10 + c).copied().unwrap_or(0) as f32;
                let v01 = src.get(p01 + c).copied().unwrap_or(0) as f32;
                let v11 = src.get(p11 + c).copied().unwrap_or(0) as f32;

                let top = v00 + dx * (v10 - v00);
                let bot = v01 + dx * (v11 - v01);
                let val = top + dy * (bot - top);

                dst[dst_idx + c] = val.round().clamp(0.0, 255.0) as u8;
            }
        }
    }
    dst
}

/// Applies generated result PNG onto a new Raster Layer in the active document,
/// automatically scaling the generated image to exactly match the target selection rect.
fn apply_generated_result(
    app: &mut PhotocraftApp,
    sel_rect: DRect,
    prompt: &str,
    out_path: &str,
) {
    let bytes = match std::fs::read(out_path) {
        Ok(b) => b,
        Err(e) => {
            app.ui.gen_fill_status = Some(format!("Could not read output image: {e}"));
            return;
        }
    };

    let decoded = match photocraft_codecs::decode(&bytes) {
        Ok(img) => img,
        Err(e) => {
            app.ui.gen_fill_status = Some(format!("Could not decode output image: {e}"));
            return;
        }
    };

    let label = format!("Generative Fill: {}", prompt);
    let (w, h) = decoded.dimensions();
    let target_w = sel_rect.width().max(1) as u32;
    let target_h = sel_rect.height().max(1) as u32;

    // Convert decoded image to standardized interleaved RGBA8 (4 channels)
    // to prevent stride mismatch when decoded image is RGB (3 channels)
    let rgba8 = decoded.to_rgba8();

    // Rescale generated image to exactly match the selection dimensions
    let rescaled_bytes = if w != target_w || h != target_h {
        rescale_rgba8(&rgba8, w, h, target_w, target_h)
    } else {
        rgba8
    };

    let edit_res = app.session.edit(&label, |doc, active| {
        let fmt = photocraft_color::PixelFormat::new(doc.mode, doc.depth, true);
        let mut surf = photocraft_raster::Surface::new(fmt);
        let target_rect = photocraft_geom::Rect::new(
            sel_rect.x0,
            sel_rect.y0,
            sel_rect.x0 + target_w as i32,
            sel_rect.y0 + target_h as i32,
        );
        surf.write_interleaved(target_rect, &rescaled_bytes);
        surf.prune();

        let new_layer = photocraft_doc::Layer::new(&label, photocraft_doc::LayerContent::Raster(surf));
        *active = Some(new_layer.id);
        doc.layers.push(new_layer);
        Ok(())
    });

    if let Err(e) = edit_res {
        app.ui.gen_fill_status = Some(format!("Failed to add layer: {e}"));
    }
}
