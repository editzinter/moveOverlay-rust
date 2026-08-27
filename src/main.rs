mod capture;
mod config;
mod engine;
mod overlay;
mod vision;

use crate::capture::grabber::capture_region;
use crate::config::AppConfig;
use crate::engine::stockfish::Stockfish;
use crate::vision::board::detections_to_fen;
use crate::vision::inference::Detector;

use crossbeam_channel::{unbounded, Receiver};
use eframe::egui;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

#[cfg(target_os = "windows")]
fn apply_stealth_affinity(window_title: &str, enable: bool) {
    use windows::core::HSTRING;
    use windows::Win32::UI::WindowsAndMessaging::{
        FindWindowW, SetWindowDisplayAffinity, WINDOW_DISPLAY_AFFINITY,
    };

    unsafe {
        let title = HSTRING::from(window_title);
        if let Ok(hwnd) = FindWindowW(None, windows::core::PCWSTR(title.as_ptr())) {
            if !hwnd.is_invalid() {
                let affinity = if enable {
                    WINDOW_DISPLAY_AFFINITY(0x00000011) // WDA_EXCLUDEFROMCAPTURE
                } else {
                    WINDOW_DISPLAY_AFFINITY(0)
                };
                let _ = SetWindowDisplayAffinity(hwnd, affinity);
            }
        }
    }
}

#[cfg(not(target_os = "windows"))]
fn apply_stealth_affinity(_window_title: &str, _enable: bool) {}

fn main() {
    println!("Starting MoveOverlay Chess Assistant...");

    let config = Arc::new(Mutex::new(AppConfig::load()));
    let (move_tx, move_rx) = unbounded::<Vec<String>>();

    // Background worker thread for Vision + Stockfish Engine
    let config_clone = config.clone();
    thread::spawn(move || {
        let exe_dir = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
        let model_path = exe_dir.join("best.onnx");
        let engine_path = exe_dir.join("stockfish.exe");

        if !model_path.exists() || !engine_path.exists() {
            eprintln!("ERROR: Missing 'best.onnx' or 'stockfish.exe' in working directory.");
            return;
        }

        let mut detector = match Detector::new(model_path.to_str().unwrap()) {
            Ok(d) => d,
            Err(e) => {
                eprintln!("Detector Initialization Error: {:?}", e);
                return;
            }
        };

        let mut sf = match Stockfish::new(engine_path.to_str().unwrap()) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("Stockfish Initialization Error: {:?}", e);
                return;
            }
        };

        println!("Vision & Engine worker thread running.");

        let mut last_analyzed_fen: Option<String> = None;
        let mut candidate_fen: Option<String> = None;
        let mut candidate_count: u32 = 0;
        let mut last_running_state = false;
        let mut last_play_side = false;
        let mut last_depth = 0;
        let mut last_lines = 0;
        let mut last_region: Option<crate::config::BoardRegion> = None;

        loop {
            let (region, depth, lines, conf, play_as_black, fps, running) = {
                let c = config_clone.lock().unwrap();
                (
                    c.board_region.clone(),
                    c.stockfish_depth,
                    c.stockfish_lines,
                    c.confidence_threshold,
                    c.play_as_black,
                    c.fps,
                    c.running,
                )
            };

            // Reset cache if any critical settings change or analysis stops
            let state_changed = running != last_running_state
                || play_as_black != last_play_side
                || depth != last_depth
                || lines != last_lines
                || region != last_region;

            if state_changed {
                last_analyzed_fen = None;
                candidate_fen = None;
                candidate_count = 0;
                last_running_state = running;
                last_play_side = play_as_black;
                last_depth = depth;
                last_lines = lines;
                last_region = region.clone();
                if !running {
                    let _ = move_tx.send(Vec::new());
                }
            }

            if running {
                if let Some(r) = region {
                    if let Ok(img) = capture_region(r.x, r.y, r.width, r.height) {
                        if let Ok(detections) = detector.detect(&img, conf) {
                            if let Some(fen) = detections_to_fen(&detections, play_as_black) {
                                // 2-frame debouncing to eliminate camera/animation jitter
                                if candidate_fen.as_deref() == Some(&fen) {
                                    candidate_count += 1;
                                } else {
                                    candidate_fen = Some(fen.clone());
                                    candidate_count = 1;
                                }

                                if candidate_count >= 2 {
                                    if last_analyzed_fen.as_deref() != Some(&fen) {
                                        match sf.analyze(&fen, depth, lines) {
                                            Ok(moves) => {
                                                let _ = move_tx.send(moves);
                                                last_analyzed_fen = Some(fen);
                                            }
                                            Err(e) => {
                                                eprintln!("Stockfish Error: {:?}. Restarting...", e);
                                                if let Ok(new_sf) =
                                                    Stockfish::new(engine_path.to_str().unwrap())
                                                {
                                                    sf = new_sf;
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }

            thread::sleep(Duration::from_millis(1000 / fps.clamp(1, 30) as u64));
        }
    });

    // Global Hotkey Listener (B: Toggle Side, R: Select Region)
    let config_hotkey = config.clone();
    thread::spawn(move || {
        use rdev::{listen, EventType, Key};
        let _ = listen(move |event| {
            if let EventType::KeyPress(key) = event.event_type {
                match key {
                    Key::KeyB => {
                        let mut c = config_hotkey.lock().unwrap();
                        c.play_as_black = !c.play_as_black;
                        println!(
                            "Perspective changed to: {}",
                            if c.play_as_black { "Black" } else { "White" }
                        );
                    }
                    Key::KeyR => {
                        let mut c = config_hotkey.lock().unwrap();
                        c.request_selection = true;
                    }
                    _ => {}
                }
            }
        });
    });

    let window_title = {
        let c = config.lock().unwrap();
        c.window_title.clone()
    };

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(&window_title)
            .with_transparent(true)
            .with_decorations(false)
            .with_always_on_top()
            .with_maximized(true)
            .with_mouse_passthrough(true)
            .with_active(true),
        ..Default::default()
    };

    let config_ui = config.clone();
    let _ = eframe::run_native(
        &window_title,
        options,
        Box::new(move |cc| {
            let mut visuals = egui::Visuals::dark();
            visuals.panel_fill = egui::Color32::TRANSPARENT;
            cc.egui_ctx.set_visuals(visuals);

            Ok(Box::new(OverlayWrapper {
                config: config_ui,
                move_rx,
                current_moves: Vec::new(),
                selection_mode: false,
                selection_start: None,
                stealth_applied: false,
            }))
        }),
    );
}

struct OverlayWrapper {
    config: Arc<Mutex<AppConfig>>,
    move_rx: Receiver<Vec<String>>,
    current_moves: Vec<String>,
    selection_mode: bool,
    selection_start: Option<egui::Pos2>,
    stealth_applied: bool,
}

impl eframe::App for OverlayWrapper {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 0.0]
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        while let Ok(moves) = self.move_rx.try_recv() {
            self.current_moves = moves;
        }

        let config_for_settings = self.config.clone();

        // Settings Viewport Window
        ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of("overlay_settings_panel"),
            egui::ViewportBuilder::default()
                .with_title("MoveOverlay Control Panel")
                .with_inner_size([340.0, 520.0])
                .with_min_inner_size([320.0, 480.0])
                .with_always_on_top()
                .with_decorations(true),
            |ctx, _class| {
                let mut visuals = egui::Visuals::dark();
                visuals.override_text_color = Some(egui::Color32::from_rgb(230, 235, 245));
                visuals.widgets.noninteractive.bg_fill = egui::Color32::from_rgb(24, 26, 32);
                visuals.widgets.inactive.bg_fill = egui::Color32::from_rgb(34, 38, 48);
                visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(46, 52, 66);
                visuals.widgets.active.bg_fill = egui::Color32::from_rgb(60, 68, 86);
                ctx.set_visuals(visuals);

                egui::CentralPanel::default().show(ctx, |ui| {
                    let mut c = config_for_settings.lock().unwrap();

                    ui.add_space(4.0);
                    // Title Header
                    ui.horizontal(|ui| {
                        ui.heading("♟ MoveOverlay");
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if c.running {
                                ui.label(egui::RichText::new("● ACTIVE").color(egui::Color32::from_rgb(76, 217, 100)).strong());
                            } else {
                                ui.label(egui::RichText::new("● IDLE").color(egui::Color32::from_rgb(255, 69, 58)).strong());
                            }
                        });
                    });

                    ui.add_space(6.0);
                    ui.separator();
                    ui.add_space(6.0);

                    // 1. Play Side / Perspective Selector
                    ui.label(egui::RichText::new("PLAYING PERSPECTIVE").strong().size(11.0).color(egui::Color32::from_rgb(160, 175, 200)));
                    ui.horizontal(|ui| {
                        let white_btn = ui.selectable_label(!c.play_as_black, "♙ White (Bottom)");
                        if white_btn.clicked() {
                            c.play_as_black = false;
                        }
                        let black_btn = ui.selectable_label(c.play_as_black, "♟ Black (Bottom)");
                        if black_btn.clicked() {
                            c.play_as_black = true;
                        }
                    });
                    ui.label(egui::RichText::new("Shortcut: Press 'B' key globally to switch side").italics().size(10.5).color(egui::Color32::from_rgb(130, 140, 160)));

                    ui.add_space(8.0);
                    ui.separator();
                    ui.add_space(6.0);

                    // 2. Engine Settings
                    ui.label(egui::RichText::new("STOCKFISH ENGINE").strong().size(11.0).color(egui::Color32::from_rgb(160, 175, 200)));
                    ui.add(egui::Slider::new(&mut c.stockfish_depth, 1..=30).text("Search Depth"));
                    ui.add(egui::Slider::new(&mut c.stockfish_lines, 1..=5).text("Suggested Lines"));
                    ui.add(egui::Slider::new(&mut c.fps, 1..=10).text("Scan Rate (FPS)"));

                    ui.add_space(8.0);
                    ui.separator();
                    ui.add_space(6.0);

                    // 3. Vision & Overlay Styling
                    ui.label(egui::RichText::new("VISION & OVERLAY").strong().size(11.0).color(egui::Color32::from_rgb(160, 175, 200)));
                    ui.add(egui::Slider::new(&mut c.confidence_threshold, 0.1..=0.9).text("AI Confidence"));
                    ui.add(egui::Slider::new(&mut c.arrow_thickness, 3.0..=12.0).text("Arrow Thickness"));
                    ui.checkbox(&mut c.stealth_mode, "Anti-Capture Stealth (Exclude from OBS/Share)");

                    ui.add_space(8.0);
                    ui.separator();
                    ui.add_space(8.0);

                    // 4. Region & Action Controls
                    let region_status = if let Some(r) = &c.board_region {
                        format!("Region: [{}, {}] {}x{}", r.x, r.y, r.width, r.height)
                    } else {
                        "Region: Not Selected".to_string()
                    };
                    ui.label(egui::RichText::new(region_status).size(11.0).color(egui::Color32::from_rgb(150, 165, 185)));

                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        if ui.button("📐 Select Board (R)").clicked() {
                            c.request_selection = true;
                        }

                        if ui.button("💾 Save Settings").clicked() {
                            if c.save().is_ok() {
                                println!("Configuration saved to config.json");
                            }
                        }
                    });

                    ui.add_space(8.0);
                    // Start / Stop Main Action Button
                    let can_start = c.board_region.is_some();
                    if c.running {
                        let stop_btn = egui::Button::new(egui::RichText::new("⏹ STOP ANALYSIS").size(14.0).strong().color(egui::Color32::WHITE))
                            .fill(egui::Color32::from_rgb(200, 40, 40))
                            .min_size(egui::vec2(ui.available_width(), 34.0));
                        if ui.add(stop_btn).clicked() {
                            c.running = false;
                        }
                    } else {
                        let start_btn = egui::Button::new(egui::RichText::new("▶ START ANALYSIS").size(14.0).strong().color(egui::Color32::WHITE))
                            .fill(if can_start { egui::Color32::from_rgb(34, 150, 75) } else { egui::Color32::from_rgb(60, 70, 80) })
                            .min_size(egui::vec2(ui.available_width(), 34.0));
                        if ui.add_enabled(can_start, start_btn).clicked() {
                            c.running = true;
                        }
                    }
                });
            },
        );

        // Apply Stealth affinity once or on toggle change
        {
            let config_guard = self.config.lock().unwrap();
            if config_guard.stealth_mode && !self.stealth_applied {
                apply_stealth_affinity(&config_guard.window_title, true);
                self.stealth_applied = true;
            } else if !config_guard.stealth_mode && self.stealth_applied {
                apply_stealth_affinity(&config_guard.window_title, false);
                self.stealth_applied = false;
            }
        }

        // Overlay & Selection Rendering
        egui::CentralPanel::default()
            .frame(egui::Frame::none())
            .show(ctx, |ui| {
                let painter = ui.painter();
                {
                    let mut c = self.config.lock().unwrap();
                    if c.request_selection {
                        self.selection_mode = true;
                        c.request_selection = false;
                    }
                }

                if self.selection_mode {
                    ctx.send_viewport_cmd(egui::ViewportCommand::MousePassthrough(false));
                    painter.rect_filled(ui.max_rect(), 0.0, egui::Color32::from_black_alpha(160));
                    painter.text(
                        ui.max_rect().center(),
                        egui::Align2::CENTER_CENTER,
                        "CLICK AND DRAG ACROSS CHESSBOARD TO SELECT",
                        egui::FontId::proportional(26.0),
                        egui::Color32::WHITE,
                    );

                    let response = ui.interact(
                        ui.max_rect(),
                        egui::Id::new("selection_drag"),
                        egui::Sense::drag(),
                    );
                    if response.drag_started() {
                        self.selection_start = response.interact_pointer_pos();
                    }
                    if let Some(start) = self.selection_start {
                        if let Some(current) = response.interact_pointer_pos() {
                            let rect = egui::Rect::from_two_pos(start, current);
                            painter.rect_stroke(
                                rect,
                                0.0,
                                egui::Stroke::new(2.5, egui::Color32::from_rgb(0, 230, 118)),
                            );
                            if response.drag_stopped() {
                                let mut c = self.config.lock().unwrap();
                                c.board_region = Some(crate::config::BoardRegion {
                                    x: rect.min.x as u32,
                                    y: rect.min.y as u32,
                                    width: rect.width() as u32,
                                    height: rect.height() as u32,
                                });
                                let _ = c.save();
                                self.selection_mode = false;
                                self.selection_start = None;
                            }
                        }
                    }
                } else {
                    ctx.send_viewport_cmd(egui::ViewportCommand::MousePassthrough(true));
                    let (region, play_as_black, thickness, running) = {
                        let c = self.config.lock().unwrap();
                        (
                            c.board_region.clone(),
                            c.play_as_black,
                            c.arrow_thickness,
                            c.running,
                        )
                    };

                    if running {
                        if let Some(region) = region {
                            let rect = egui::Rect::from_min_size(
                                egui::pos2(region.x as f32, region.y as f32),
                                egui::vec2(region.width as f32, region.height as f32),
                            );

                            for (i, m) in self.current_moves.iter().enumerate() {
                                let color = match i {
                                    0 => egui::Color32::from_rgba_unmultiplied(0, 230, 118, 240), // 1st Best: Emerald
                                    1 => egui::Color32::from_rgba_unmultiplied(255, 193, 7, 215), // 2nd Best: Amber
                                    2 => egui::Color32::from_rgba_unmultiplied(0, 229, 255, 185), // 3rd Best: Vivid Cyan
                                    _ => egui::Color32::from_rgba_unmultiplied(186, 104, 200, 140),
                                };
                                crate::overlay::window::draw_arrow(
                                    painter,
                                    rect,
                                    m,
                                    color,
                                    play_as_black,
                                    thickness,
                                );
                            }
                        }
                    }
                }
            });

        ctx.request_repaint_after(Duration::from_millis(50));
    }
}
