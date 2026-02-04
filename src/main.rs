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

fn main() {
    println!("Starting Chess Overlay...");

    let config = Arc::new(Mutex::new(AppConfig::load()));
    let (move_tx, move_rx) = unbounded::<Vec<String>>();

    // Background worker thread for Vision + Stockfish
    let config_clone = config.clone();
    thread::spawn(move || {
        let exe_dir = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
        let model_path = exe_dir.join("best.onnx");
        let engine_path = exe_dir.join("stockfish.exe");

        if !model_path.exists() || !engine_path.exists() {
            println!("ERROR: Essential files missing");
            return;
        }

        let mut detector = match Detector::new(model_path.to_str().unwrap()) {
            Ok(d) => d,
            Err(e) => {
                println!("ERROR: {:?}", e);
                return;
            }
        };

        let mut sf = match Stockfish::new(engine_path.to_str().unwrap()) {
            Ok(s) => s,
            Err(e) => {
                println!("ERROR: {:?}", e);
                return;
            }
        };

        println!("Worker thread ready");
        loop {
            let (region, depth, lines, conf, show_white, fps, running) = {
                let c = config_clone.lock().unwrap();
                (
                    c.board_region.clone(),
                    c.stockfish_depth,
                    c.stockfish_lines,
                    c.confidence_threshold,
                    c.show_white_moves,
                    c.fps,
                    c.running,
                )
            };

            if running {
                if let Some(r) = region {
                    if let Ok(img) = capture_region(r.x, r.y, r.width, r.height) {
                        if let Ok(detections) = detector.detect(&img, conf) {
                            if let Some(fen) = detections_to_fen(&detections, show_white) {
                                // Add a retry mechanism for Stockfish
                                match sf.analyze(&fen, depth, lines) {
                                    Ok(moves) => {
                                        let _ = move_tx.send(moves);
                                    }
                                    Err(e) => {
                                        println!("Stockfish Error: {:?}. Attempting restart...", e);
                                        if let Ok(new_sf) =
                                            Stockfish::new(engine_path.to_str().unwrap())
                                        {
                                            sf = new_sf;
                                        }
                                    }
                                }
                            } else {
                                // Illegal FEN (likely missing King in vision)
                                // Send empty moves to clear old arrows if vision is consistently bad
                                // let _ = move_tx.send(vec![]);
                            }
                        }
                    }
                }
            }
            thread::sleep(Duration::from_millis(1000 / fps.max(1) as u64));
        }
    });

    // Global Hotkey Listener
    let config_hotkey = config.clone();
    thread::spawn(move || {
        use rdev::{listen, EventType};
        listen(move |event| {
            if let EventType::KeyPress(key) = event.event_type {
                if format!("{:?}", key) == "KeyB" {
                    let mut c = config_hotkey.lock().unwrap();
                    c.show_white_moves = !c.show_white_moves;
                    println!(
                        "Toggled side: {}",
                        if c.show_white_moves { "White" } else { "Black" }
                    );
                }
            }
        })
        .expect("Failed to listen for hotkeys");
    });

    // Run Overlay UI
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
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
        "Chess Overlay Visuals",
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
        ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of("settings_window"),
            egui::ViewportBuilder::default()
                .with_title("Chess Overlay Settings")
                .with_inner_size([300.0, 400.0])
                .with_always_on_top()
                .with_decorations(true),
            move |ctx, _class| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let mut c = config_for_settings.lock().unwrap();

                    ui.heading("Analysis Control");
                    ui.horizontal(|ui| {
                        if c.running {
                            if ui.button("⏹ STOP").clicked() {
                                c.running = false;
                            }
                            ui.label("🟢 Running");
                        } else {
                            let can_start = c.board_region.is_some();
                            if ui
                                .add_enabled(can_start, egui::Button::new("▶ START"))
                                .clicked()
                            {
                                c.running = true;
                            }
                            if !can_start {
                                ui.label("⚠ Select region");
                            } else {
                                ui.label("🔴 Stopped");
                            }
                        }
                    });

                    ui.separator();
                    ui.label("Stockfish Settings");
                    ui.add(egui::Slider::new(&mut c.stockfish_depth, 1..=30).text("Depth"));
                    ui.add(egui::Slider::new(&mut c.stockfish_lines, 1..=5).text("Lines"));

                    ui.separator();
                    ui.label("Vision Settings");
                    ui.add(
                        egui::Slider::new(&mut c.confidence_threshold, 0.1..=1.0)
                            .text("Confidence"),
                    );
                    ui.checkbox(&mut c.show_white_moves, "Show White (B key)");

                    ui.separator();
                    if ui.button("📐 Select Board Region").clicked() {
                        c.request_selection = true;
                    }

                    if ui.button("💾 Save Settings").clicked() {
                        let _ = c.save();
                    }
                });
            },
        );

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
                    painter.rect_filled(ui.max_rect(), 0.0, egui::Color32::from_black_alpha(150));
                    painter.text(
                        ui.max_rect().center(),
                        egui::Align2::CENTER_CENTER,
                        "DRAG TO SELECT BOARD",
                        egui::FontId::proportional(30.0),
                        egui::Color32::WHITE,
                    );

                    let response = ui.interact(
                        ui.max_rect(),
                        egui::Id::new("selection"),
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
                                egui::Stroke::new(2.0, egui::Color32::RED),
                            );
                            if response.drag_stopped() {
                                let mut c = self.config.lock().unwrap();
                                c.board_region = Some(crate::config::BoardRegion {
                                    x: rect.min.x as u32,
                                    y: rect.min.y as u32,
                                    width: rect.width() as u32,
                                    height: rect.height() as u32,
                                });
                                self.selection_mode = false;
                                self.selection_start = None;
                            }
                        }
                    }
                } else {
                    ctx.send_viewport_cmd(egui::ViewportCommand::MousePassthrough(true));
                    let config = self.config.lock().unwrap();
                    if let Some(region) = &config.board_region {
                        let rect = egui::Rect::from_min_size(
                            egui::pos2(region.x as f32, region.y as f32),
                            egui::vec2(region.width as f32, region.height as f32),
                        );
                        for (i, m) in self.current_moves.iter().enumerate() {
                            let opacity = match i {
                                0 => 255,
                                1 => 160,
                                _ => 80,
                            };
                            let color = egui::Color32::from_rgba_unmultiplied(0, 255, 0, opacity);
                            crate::overlay::window::draw_arrow(painter, rect, m, color);
                        }
                    }
                }
            });
        ctx.request_repaint();
    }
}
