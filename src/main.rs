use eframe::{egui, App, Frame, NativeOptions};
use egui::{Color32, Pos2, Stroke};
use image::imageops::FilterType;
use image::GenericImageView;
use ndarray::Array4;
use ort::session::{builder::GraphOptimizationLevel, Session};
use ort::value::Value;
use std::path::Path;
use std::time::{Duration, Instant};
use xcap::Monitor;

mod chess_logic;
mod config;
mod engine;
mod yolo;

use chess_logic::{detect_orientation, detections_to_fen, move_to_rect, Orientation};
use config::{load_config, save_config, AppConfig, Region};
use engine::Stockfish;

enum AppState {
    Menu,
    Selecting {
        start_pos: Option<Pos2>,
        current_pos: Option<Pos2>,
    },
    Overlay,
}

struct ChessApp {
    config: AppConfig,
    state: AppState,

    // Components
    engine: Option<Stockfish>,
    session: Option<Session>,

    // Overlay State
    last_arrows: Option<Vec<((f32, f32), (f32, f32), Color32)>>,
    last_analysis_time: Instant,
    frame_count: u64,
}

impl ChessApp {
    fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        let config = load_config();

        Self {
            config,
            state: AppState::Menu,
            engine: None,
            session: None,
            last_arrows: None,
            last_analysis_time: Instant::now(),
            frame_count: 0,
        }
    }

    fn init_overlay(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        if self.session.is_none() {
            let model_path = Path::new("best.onnx");
            let session = Session::builder()?
                .with_optimization_level(GraphOptimizationLevel::Level3)?
                .with_intra_threads(4)?
                .commit_from_file(model_path)?;
            self.session = Some(session);
        }

        if self.engine.is_none() {
            let mut engine = Stockfish::new("stockfish.exe")?;
            engine.set_option("MultiPV", &self.config.stockfish.multipv.to_string())?;
            self.engine = Some(engine);
        } else if let Some(engine) = &mut self.engine {
            // Update settings if already exists
            engine.set_option("MultiPV", &self.config.stockfish.multipv.to_string())?;
        }

        Ok(())
    }

    fn run_analysis(&mut self) {
        if self.config.region.is_none() {
            return;
        }
        let region = self.config.region.as_ref().unwrap();

        let monitors = Monitor::all().unwrap_or_default();
        let monitor = monitors.first();
        if monitor.is_none() {
            return;
        }
        let monitor = monitor.unwrap();

        let full_image = match monitor.capture_image() {
            Ok(img) => img,
            Err(e) => {
                eprintln!("Screenshot failed: {}", e);
                return;
            }
        };

        let img_width = full_image.width();
        let img_height = full_image.height();

        let r_x = (region.x as u32).min(img_width.saturating_sub(1));
        let r_y = (region.y as u32).min(img_height.saturating_sub(1));
        let r_w = region.width.min(img_width.saturating_sub(r_x));
        let r_h = region.height.min(img_height.saturating_sub(r_y));

        if r_w == 0 || r_h == 0 {
            return;
        }

        let cropped = full_image.view(r_x, r_y, r_w, r_h).to_image();
        let img = image::DynamicImage::ImageRgba8(cropped).to_rgb8();
        let resized = image::imageops::resize(&img, 640, 640, FilterType::Triangle);

        let mut input_tensor = Array4::<f32>::zeros((1, 3, 640, 640));
        for (x, y, pixel) in resized.enumerate_pixels() {
            let [r, g, b] = pixel.0;
            input_tensor[[0, 0, y as usize, x as usize]] = (r as f32) / 255.0;
            input_tensor[[0, 1, y as usize, x as usize]] = (g as f32) / 255.0;
            input_tensor[[0, 2, y as usize, x as usize]] = (b as f32) / 255.0;
        }

        let session = self.session.as_mut().unwrap();
        let detections = {
            let input_value = Value::from_array(input_tensor).unwrap();
            let inputs = ort::inputs!["images" => input_value];

            let outputs = match session.run(inputs) {
                Ok(o) => o,
                Err(e) => {
                    eprintln!("Inference error: {}", e);
                    return;
                }
            };

            let output = match outputs.get("output0") {
                Some(o) => o,
                None => return,
            };

            let (shape, data) = match output.try_extract_tensor::<f32>() {
                Ok(t) => t,
                Err(_) => return,
            };
            let shape_usize: Vec<usize> = shape.iter().map(|&x| x as usize).collect();
            let output_view = ndarray::ArrayView::from_shape(shape_usize, data).unwrap();

            yolo::postprocess(
                output_view.into_dimensionality::<ndarray::Ix3>().unwrap(),
                0.25,
                0.45,
            )
        };

        // Logic
        let (_, board_rect) = detections_to_fen(&detections, Orientation::WhiteBottom);
        let orientation = detect_orientation(&detections, board_rect);
        let (placement, board_rect) = detections_to_fen(&detections, orientation);

        let fen_white = format!("{} w - - 0 1", placement);
        let fen_black = format!("{} b - - 0 1", placement);

        self.last_arrows = Some(Vec::new());

        let region_x = region.x as f32;
        let region_y = region.y as f32;
        let scale_x = r_w as f32;
        let scale_y = r_h as f32;

        // White
        if let Err(e) = self.analyze_and_draw(
            fen_white,
            board_rect,
            orientation,
            region_x,
            region_y,
            scale_x,
            scale_y,
            Color32::GREEN,
        ) {
            println!("Engine Error (White): {}", e);
            if let Some(eng) = &mut self.engine {
                let _ = eng.restart();
                // Restore settings
                let _ = eng.set_option("MultiPV", &self.config.stockfish.multipv.to_string());
            }
        }

        // Black
        if let Err(e) = self.analyze_and_draw(
            fen_black,
            board_rect,
            orientation,
            region_x,
            region_y,
            scale_x,
            scale_y,
            Color32::RED,
        ) {
            println!("Engine Error (Black): {}", e);
        }
    }

    fn analyze_and_draw(
        &mut self,
        fen: String,
        board_rect: chess_logic::Rect,
        orientation: Orientation,
        rx: f32,
        ry: f32,
        sx: f32,
        sy: f32,
        color: Color32,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let engine = self.engine.as_mut().ok_or("Engine not initialized")?;
        let top_moves = engine.get_top_moves(&fen, self.config.stockfish.depth)?;

        for best_move in top_moves {
            if let Some((x1, y1, x2, y2)) = move_to_rect(&best_move, board_rect, orientation) {
                if let Some(arrows) = &mut self.last_arrows {
                    arrows.push((
                        (rx + x1 * sx, ry + y1 * sy),
                        (rx + x2 * sx, ry + y2 * sy),
                        color,
                    ));
                }
            }
        }
        Ok(())
    }
}

impl App for ChessApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut Frame) {
        let mut next_state = None;

        match self.state {
            AppState::Menu => {
                // Window Settings
                ctx.send_viewport_cmd(egui::ViewportCommand::Decorations(true));
                ctx.send_viewport_cmd(egui::ViewportCommand::Transparent(false));
                ctx.send_viewport_cmd(egui::ViewportCommand::MousePassthrough(false));
                ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(false));

                let transition = egui::CentralPanel::default()
                    .show(ctx, |ui| -> Option<AppState> {
                        ui.heading("Chess Overlay Launcher");
                        ui.separator();

                        ui.group(|ui| {
                            ui.label("Stockfish Settings");
                            ui.add(
                                egui::Slider::new(&mut self.config.stockfish.depth, 1..=30)
                                    .text("Depth"),
                            );
                            ui.add(
                                egui::Slider::new(&mut self.config.stockfish.multipv, 1..=5)
                                    .text("MultiPV (Lines)"),
                            );
                        });

                        if let Some(r) = self.config.region {
                            ui.label(format!(
                                "Region Selected: {}x{} at ({},{})",
                                r.width, r.height, r.x, r.y
                            ));
                        } else {
                            ui.label("No Region Selected");
                        }

                        if ui.button("Select Region").clicked() {
                            // Prepare for selection mode
                            ctx.send_viewport_cmd(egui::ViewportCommand::Decorations(false));
                            ctx.send_viewport_cmd(egui::ViewportCommand::Transparent(true));
                            ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(true));
                            return Some(AppState::Selecting {
                                start_pos: None,
                                current_pos: None,
                            });
                        }

                        if ui.button("Start Overlay").clicked() {
                            if self.config.region.is_some() {
                                if let Err(e) = self.init_overlay() {
                                    eprintln!("Failed to init overlay: {}", e);
                                } else {
                                    let _ = save_config(&self.config);

                                    // Overlay Setup
                                    ctx.send_viewport_cmd(egui::ViewportCommand::Decorations(
                                        false,
                                    ));
                                    ctx.send_viewport_cmd(egui::ViewportCommand::Transparent(true));
                                    ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(true));
                                    // Mouse passthrough will be enabled after a few frames in update loop
                                    return Some(AppState::Overlay);
                                }
                            }
                        }
                        None
                    })
                    .inner;

                if let Some(s) = transition {
                    if let AppState::Overlay = s {
                        self.frame_count = 0;
                    }
                    next_state = Some(s);
                }
            }
            AppState::Selecting {
                ref mut start_pos,
                ref mut current_pos,
            } => {
                let panel_frame = egui::Frame {
                    fill: egui::Color32::from_rgba_premultiplied(0, 0, 0, 1), // Nearly transparent
                    ..Default::default()
                };

                let selected_region = egui::CentralPanel::default()
                    .frame(panel_frame)
                    .show(ctx, |ui| -> Option<Region> {
                        let screen_rect = ui.max_rect();
                        ui.painter().text(
                            screen_rect.center(),
                            egui::Align2::CENTER_CENTER,
                            "Click and Drag to Select Region",
                            egui::FontId::proportional(30.0),
                            Color32::WHITE,
                        );

                        let pointer = ctx.input(|i| i.pointer.clone());
                        if pointer.primary_pressed() {
                            if start_pos.is_none() {
                                *start_pos = pointer.press_origin();
                            }
                            *current_pos = pointer.interact_pos();
                        } else if pointer.primary_released() {
                            if let (Some(start), Some(current)) = (*start_pos, *current_pos) {
                                let rect = egui::Rect::from_two_pos(start, current);
                                if rect.width() > 10.0 && rect.height() > 10.0 {
                                    return Some(Region {
                                        x: rect.min.x as i32,
                                        y: rect.min.y as i32,
                                        width: rect.width() as u32,
                                        height: rect.height() as u32,
                                    });
                                }
                            }
                            *start_pos = None;
                            *current_pos = None;
                        } else if start_pos.is_some() {
                            *current_pos = pointer.interact_pos();
                        }

                        if let (Some(start), Some(current)) = (*start_pos, *current_pos) {
                            let rect = egui::Rect::from_two_pos(start, current);
                            ui.painter().rect_filled(
                                rect,
                                0.0,
                                Color32::from_rgba_unmultiplied(100, 100, 255, 50),
                            );
                            ui.painter()
                                .rect_stroke(rect, 0.0, Stroke::new(2.0, Color32::YELLOW));
                        }
                        None
                    })
                    .inner;

                if let Some(r) = selected_region {
                    self.config.region = Some(r);
                    let _ = save_config(&self.config);
                    next_state = Some(AppState::Menu);
                }
            }
            AppState::Overlay => {
                // Logic
                self.frame_count += 1;
                if self.frame_count == 10 {
                    println!("Enabling Mouse Passthrough...");
                    ctx.send_viewport_cmd(egui::ViewportCommand::MousePassthrough(true));
                }

                // Hotkey to return to menu (Insert Key)
                if ctx.input(|i| i.key_pressed(egui::Key::Insert)) {
                    next_state = Some(AppState::Menu);
                } else {
                    if self.last_analysis_time.elapsed() >= Duration::from_millis(500) {
                        self.run_analysis();
                        self.last_analysis_time = Instant::now();
                    }

                    let painter = ctx.layer_painter(egui::LayerId::new(
                        egui::Order::Foreground,
                        egui::Id::new("overlay"),
                    ));
                    painter.text(
                        Pos2::new(50.0, 50.0),
                        egui::Align2::LEFT_TOP,
                        "Overlay Active (Press INSERT for Menu)",
                        egui::FontId::proportional(20.0),
                        Color32::WHITE,
                    );

                    let ppp = ctx.pixels_per_point();
                    if let Some(arrows) = &self.last_arrows {
                        for ((x1, y1), (x2, y2), color) in arrows {
                            let start = Pos2::new(*x1 / ppp, *y1 / ppp);
                            let end = Pos2::new(*x2 / ppp, *y2 / ppp);
                            let vec = end - start;
                            painter.arrow(start, vec, Stroke::new(6.0, *color));
                        }
                    }

                    ctx.request_repaint_after(Duration::from_millis(100));
                }
            }
        }

        if let Some(s) = next_state {
            self.state = s;
        }
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        match self.state {
            AppState::Menu => [0.1, 0.1, 0.1, 1.0], // Dark gray background for menu
            _ => [0.0, 0.0, 0.0, 0.0],              // Transparent for overlay/selecting
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let _ = ort::init().with_name("chess_overlay").commit();

    let options = NativeOptions {
        // Start as a normal window for the menu
        viewport: egui::ViewportBuilder::default()
            .with_decorations(true)
            .with_transparent(true) // ENABLED globally to allow switching
            .with_always_on_top() // Force ALWAYS ON TOP from start
            .with_inner_size([400.0, 300.0]),
        renderer: eframe::Renderer::Glow,
        ..Default::default()
    };

    println!("Starting Chess Overlay...");
    eframe::run_native(
        "Chess Overlay",
        options,
        Box::new(|cc| Box::new(ChessApp::new(cc))),
    )
    .map_err(|e| format!("{}", e))?;

    Ok(())
}
