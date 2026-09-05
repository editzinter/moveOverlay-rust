mod capture;
mod config;
mod engine;
mod overlay;
mod vision;

use crate::capture::grabber::capture_region;
use crate::config::AppConfig;
use crate::engine::stockfish::Stockfish;
use crate::vision::inference::Detector;

use crossbeam_channel::{unbounded, Receiver};
use eframe::egui;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

#[cfg(target_os = "windows")]
pub fn apply_stealth_affinity(window_title: &str, enable: bool) {
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

#[cfg(target_os = "windows")]
fn get_physical_cursor_pos() -> Option<egui::Pos2> {
    use windows::Win32::Foundation::POINT;
    use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;
    unsafe {
        let mut pt = POINT { x: 0, y: 0 };
        if GetCursorPos(&mut pt).is_ok() {
            Some(egui::pos2(pt.x as f32, pt.y as f32))
        } else {
            None
        }
    }
}

#[cfg(not(target_os = "windows"))]
fn get_physical_cursor_pos() -> Option<egui::Pos2> {
    None
}

#[cfg(target_os = "windows")]
fn get_window_client_origin(window_title: &str) -> (i32, i32) {
    use windows::core::HSTRING;
    use windows::Win32::Foundation::POINT;
    use windows::Win32::Graphics::Gdi::ClientToScreen;
    use windows::Win32::UI::WindowsAndMessaging::FindWindowW;

    unsafe {
        let title = HSTRING::from(window_title);
        if let Ok(hwnd) = FindWindowW(None, windows::core::PCWSTR(title.as_ptr())) {
            if !hwnd.is_invalid() {
                let mut pt = POINT { x: 0, y: 0 };
                if ClientToScreen(hwnd, &mut pt).as_bool() {
                    return (pt.x, pt.y);
                }
            }
        }
    }
    (0, 0)
}

#[cfg(not(target_os = "windows"))]
fn get_window_client_origin(_window_title: &str) -> (i32, i32) {
    (0, 0)
}

#[derive(Clone, Debug, PartialEq)]
pub enum WorkerStatus {
    Starting,
    Ready,
    MissingAssets {
        model_missing: bool,
        engine_missing: bool,
        search_path: String,
    },
    InitError(String),
}

fn lock_config(config: &Arc<Mutex<AppConfig>>) -> std::sync::MutexGuard<'_, AppConfig> {
    config
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn main() {
    #[cfg(target_os = "windows")]
    unsafe {
        use windows::Win32::UI::HiDpi::{
            SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
        };
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }

    println!("Starting MoveOverlay Chess Assistant...");

    let initial_config = AppConfig::load();
    let config = Arc::new(Mutex::new(initial_config));
    let (move_tx, move_rx) = unbounded::<Vec<String>>();
    let worker_status = Arc::new(Mutex::new(WorkerStatus::Starting));

    // Background worker thread for Vision + Stockfish Engine
    let config_clone = config.clone();
    let worker_status_clone = worker_status.clone();
    thread::spawn(move || {
        let (mut detector, mut sf) = loop {
            let model_path = AppConfig::get_asset_path("best.onnx");
            let engine_path = AppConfig::get_asset_path("stockfish.exe");

            let model_missing = !model_path.exists();
            let engine_missing = !engine_path.exists();

            if model_missing || engine_missing {
                {
                    let mut ws = worker_status_clone
                        .lock()
                        .unwrap_or_else(|p| p.into_inner());
                    *ws = WorkerStatus::MissingAssets {
                        model_missing,
                        engine_missing,
                        search_path: AppConfig::get_app_dir().display().to_string(),
                    };
                }
                thread::sleep(Duration::from_millis(1000));
                continue;
            }

            let d_res = Detector::new(model_path.to_str().unwrap_or("best.onnx"));
            let sf_res = Stockfish::new(engine_path.to_str().unwrap_or("stockfish.exe"));

            match (d_res, sf_res) {
                (Ok(d), Ok(s)) => {
                    let mut ws = worker_status_clone
                        .lock()
                        .unwrap_or_else(|p| p.into_inner());
                    *ws = WorkerStatus::Ready;
                    println!("Vision & Engine worker thread initialized and ready.");
                    break (d, s);
                }
                (Err(e), _) => {
                    eprintln!("Detector Initialization Error: {:?}", e);
                    let mut ws = worker_status_clone
                        .lock()
                        .unwrap_or_else(|p| p.into_inner());
                    *ws = WorkerStatus::InitError(format!("Vision Model Error: {}", e));
                    thread::sleep(Duration::from_millis(2000));
                }
                (_, Err(e)) => {
                    eprintln!("Stockfish Initialization Error: {:?}", e);
                    let mut ws = worker_status_clone
                        .lock()
                        .unwrap_or_else(|p| p.into_inner());
                    *ws = WorkerStatus::InitError(format!("Stockfish Error: {}", e));
                    thread::sleep(Duration::from_millis(2000));
                }
            }
        };

        let engine_path = AppConfig::get_asset_path("stockfish.exe");

        println!("Vision & Engine worker thread running.");

        let mut tracker = crate::vision::board::GameStateTracker::new();
        let mut last_analyzed_fen: Option<String> = None;
        let mut last_running_state = false;
        let mut last_play_side = false;
        let mut last_depth = 0;
        let mut last_lines = 0;
        let mut last_time_limit_ms = 0;
        let mut last_region: Option<crate::config::BoardRegion> = None;
        let mut invalid_detection_count: u32 = 0;

        loop {
            let (region, depth, lines, time_limit_ms, conf, play_as_black, fps, running) = {
                let c = lock_config(&config_clone);
                (
                    c.board_region.clone(),
                    c.stockfish_depth,
                    c.stockfish_lines,
                    c.stockfish_time_ms,
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
                || time_limit_ms != last_time_limit_ms
                || region != last_region;

            if state_changed {
                tracker.reset();
                last_analyzed_fen = None;
                last_running_state = running;
                last_play_side = play_as_black;
                last_depth = depth;
                last_lines = lines;
                last_time_limit_ms = time_limit_ms;
                last_region = region.clone();
                invalid_detection_count = 0;
                let _ = move_tx.send(Vec::new());
            }

            if running {
                if let Some(r) = region {
                    if r.width > 0 && r.height > 0 {
                        let capture_result = capture_region(r.x, r.y, r.width, r.height);

                        if let Ok(img) = capture_result {
                            if let Ok(detections) = detector.detect(&img, conf) {
                                if let Some(board) = crate::vision::board::detections_to_board(
                                    &detections,
                                    play_as_black,
                                ) {
                                    invalid_detection_count = 0;
                                    if let Some(fen) = tracker.update(board, play_as_black) {
                                        if last_analyzed_fen.as_deref() != Some(&fen) {
                                            // Instantly clear stale arrows from previous position
                                            let _ = move_tx.send(Vec::new());
                                            match sf.analyze(&fen, depth, lines, time_limit_ms) {
                                                Ok(raw_moves) => {
                                                    let valid_moves = crate::vision::board::validate_moves_for_side(&fen, &raw_moves, play_as_black);
                                                    println!(
                                                        "▶ Board FEN: {} | Best moves: {:?}",
                                                        fen, valid_moves
                                                    );
                                                    let _ = move_tx.send(valid_moves);
                                                    last_analyzed_fen = Some(fen);
                                                }
                                                Err(e) => {
                                                    eprintln!(
                                                        "Stockfish Error: {:?}. Restarting...",
                                                        e
                                                    );
                                                    if let Ok(new_sf) = Stockfish::new(
                                                        engine_path
                                                            .to_str()
                                                            .unwrap_or("stockfish.exe"),
                                                    ) {
                                                        sf = new_sf;
                                                    }
                                                }
                                            }
                                        }
                                    }
                                } else {
                                    invalid_detection_count += 1;
                                    if invalid_detection_count >= 3 && last_analyzed_fen.is_some() {
                                        let _ = move_tx.send(Vec::new());
                                        last_analyzed_fen = None;
                                        tracker.reset();
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

    let selection_active = Arc::new(AtomicBool::new(false));
    let hotkey_toggle_side = Arc::new(AtomicBool::new(false));
    let hotkey_select_region = Arc::new(AtomicBool::new(false));

    // Zero-lag Global Hotkey Listener (uses lock-free atomics to protect Windows hook latency)
    let toggle_side_hook = hotkey_toggle_side.clone();
    let select_region_hook = hotkey_select_region.clone();
    thread::spawn(move || {
        use rdev::{listen, EventType, Key};
        let _ = listen(move |event| {
            if let EventType::KeyPress(key) = event.event_type {
                match key {
                    Key::KeyB => {
                        toggle_side_hook.store(true, Ordering::Relaxed);
                    }
                    Key::KeyR => {
                        select_region_hook.store(true, Ordering::Relaxed);
                    }
                    _ => {}
                }
            }
        });
    });

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("♟ MoveOverlay")
            .with_transparent(true)
            .with_decorations(false)
            .with_always_on_top()
            .with_maximized(true)
            .with_active(true)
            .with_mouse_passthrough(false),
        ..Default::default()
    };

    let config_ui = config.clone();
    let selection_ui = selection_active.clone();
    let worker_status_ui = worker_status.clone();
    let _ = eframe::run_native(
        "♟ MoveOverlay",
        options,
        Box::new(move |cc| {
            let mut visuals = egui::Visuals::dark();
            visuals.panel_fill = egui::Color32::TRANSPARENT;
            visuals.override_text_color = Some(egui::Color32::from_rgb(230, 235, 245));
            visuals.widgets.noninteractive.bg_fill = egui::Color32::from_rgb(24, 26, 32);
            visuals.widgets.inactive.bg_fill = egui::Color32::from_rgb(34, 38, 48);
            visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(46, 52, 66);
            visuals.widgets.active.bg_fill = egui::Color32::from_rgb(60, 68, 86);
            cc.egui_ctx.set_visuals(visuals);

            Ok(Box::new(OverlayWrapper {
                config: config_ui,
                worker_status: worker_status_ui,
                move_rx,
                current_moves: Vec::new(),
                selection_active: selection_ui,
                hotkey_toggle_side,
                hotkey_select_region,
                drag_start: None,
                last_stealth_applied: None,
                save_feedback_timer: None,
                control_panel_rect: None,
                last_mouse_passthrough: None,
            }))
        }),
    );
}

struct OverlayWrapper {
    config: Arc<Mutex<AppConfig>>,
    worker_status: Arc<Mutex<WorkerStatus>>,
    move_rx: Receiver<Vec<String>>,
    current_moves: Vec<String>,
    selection_active: Arc<AtomicBool>,
    hotkey_toggle_side: Arc<AtomicBool>,
    hotkey_select_region: Arc<AtomicBool>,
    drag_start: Option<egui::Pos2>,
    last_stealth_applied: Option<bool>,
    save_feedback_timer: Option<Instant>,
    control_panel_rect: Option<egui::Rect>,
    last_mouse_passthrough: Option<bool>,
}

impl eframe::App for OverlayWrapper {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 0.0]
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        while let Ok(moves) = self.move_rx.try_recv() {
            self.current_moves = moves;
        }

        // Process global hotkey triggers
        if self.hotkey_toggle_side.swap(false, Ordering::Relaxed) {
            let mut c = lock_config(&self.config);
            c.play_as_black = !c.play_as_black;
            println!(
                "Perspective changed to: {}",
                if c.play_as_black { "Black" } else { "White" }
            );
        }

        if self.hotkey_select_region.swap(false, Ordering::Relaxed) {
            self.selection_active.store(true, Ordering::SeqCst);
        }

        let is_selecting = self.selection_active.load(Ordering::SeqCst);

        let ppp = ctx.pixels_per_point();
        let win_origin = get_window_client_origin("♟ MoveOverlay");
        let is_cursor_over_panel = if let (Some(panel_rect), Some(phys_pos)) =
            (self.control_panel_rect, get_physical_cursor_pos())
        {
            let logical_pos = egui::pos2(
                (phys_pos.x - win_origin.0 as f32) / ppp,
                (phys_pos.y - win_origin.1 as f32) / ppp,
            );
            panel_rect.expand(12.0).contains(logical_pos)
        } else {
            true // default to interactive on first frames
        };

        let wants_mouse = is_selecting
            || is_cursor_over_panel
            || ctx.is_pointer_over_area()
            || ctx.wants_pointer_input()
            || self.drag_start.is_some();

        let passthrough = !wants_mouse;
        if self.last_mouse_passthrough != Some(passthrough) {
            ctx.send_viewport_cmd(egui::ViewportCommand::MousePassthrough(passthrough));
            self.last_mouse_passthrough = Some(passthrough);
        }

        // Synchronize Stealth Mode display affinity with OS window
        {
            let c = lock_config(&self.config);
            if self.last_stealth_applied != Some(c.stealth_mode) {
                apply_stealth_affinity("♟ MoveOverlay", c.stealth_mode);
                self.last_stealth_applied = Some(c.stealth_mode);
            }
        }

        // Control Panel Window (floating, movable)
        if !is_selecting {
            let win_resp = egui::Window::new("♟ MoveOverlay Control Panel")
                .default_pos(egui::pos2(40.0, 40.0))
                .default_size(egui::vec2(340.0, 520.0))
                .resizable(true)
                .collapsible(true)
                .show(ctx, |ui| {
                    let mut c = lock_config(&self.config);

                    if c.request_selection {
                        self.selection_active.store(true, Ordering::SeqCst);
                        c.request_selection = false;
                    }

                    ui.add_space(4.0);
                    // Title Header
                    ui.horizontal(|ui| {
                        ui.heading("♟ MoveOverlay");
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if c.running {
                                ui.label(
                                    egui::RichText::new("● ACTIVE")
                                        .color(egui::Color32::from_rgb(76, 217, 100))
                                        .strong(),
                                );
                            } else {
                                ui.label(
                                    egui::RichText::new("● IDLE")
                                        .color(egui::Color32::from_rgb(255, 69, 58))
                                        .strong(),
                                );
                            }
                        });
                    });

                    ui.add_space(6.0);
                    ui.separator();

                    let status = self
                        .worker_status
                        .lock()
                        .unwrap_or_else(|p| p.into_inner())
                        .clone();

                    match &status {
                        WorkerStatus::Starting => {
                            ui.add_space(4.0);
                            ui.horizontal(|ui| {
                                ui.spinner();
                                ui.label(
                                    egui::RichText::new("Initializing vision & engine...")
                                        .color(egui::Color32::LIGHT_GRAY)
                                        .size(11.0),
                                );
                            });
                            ui.add_space(4.0);
                            ui.separator();
                        }
                        WorkerStatus::MissingAssets {
                            model_missing,
                            engine_missing,
                            search_path,
                        } => {
                            ui.add_space(4.0);
                            egui::Frame::none()
                                .fill(egui::Color32::from_rgb(45, 20, 20))
                                .stroke(egui::Stroke::new(
                                    1.0,
                                    egui::Color32::from_rgb(200, 60, 60),
                                ))
                                .rounding(4.0)
                                .inner_margin(8.0)
                                .show(ui, |ui| {
                                    ui.label(
                                        egui::RichText::new("⚠ MISSING REQUIRED ASSETS")
                                            .strong()
                                            .size(11.5)
                                            .color(egui::Color32::from_rgb(255, 120, 120)),
                                    );
                                    if *model_missing {
                                        ui.label(
                                            egui::RichText::new("• best.onnx (YOLO piece detector)")
                                                .size(10.5)
                                                .color(egui::Color32::from_rgb(240, 180, 180)),
                                        );
                                    }
                                    if *engine_missing {
                                        ui.label(
                                            egui::RichText::new("• stockfish.exe (Chess engine)")
                                                .size(10.5)
                                                .color(egui::Color32::from_rgb(240, 180, 180)),
                                        );
                                    }
                                    ui.add_space(2.0);
                                    ui.label(
                                        egui::RichText::new(format!("Target: {}", search_path))
                                            .size(9.5)
                                            .italics()
                                            .color(egui::Color32::from_rgb(160, 160, 160)),
                                    );
                                });
                            ui.add_space(4.0);
                            ui.separator();
                        }
                        WorkerStatus::InitError(err) => {
                            ui.add_space(4.0);
                            egui::Frame::none()
                                .fill(egui::Color32::from_rgb(45, 20, 20))
                                .stroke(egui::Stroke::new(
                                    1.0,
                                    egui::Color32::from_rgb(200, 60, 60),
                                ))
                                .rounding(4.0)
                                .inner_margin(8.0)
                                .show(ui, |ui| {
                                    ui.label(
                                        egui::RichText::new("⚠ INITIALIZATION ERROR")
                                            .strong()
                                            .size(11.5)
                                            .color(egui::Color32::from_rgb(255, 120, 120)),
                                    );
                                    ui.label(
                                        egui::RichText::new(err)
                                            .size(10.5)
                                            .color(egui::Color32::from_rgb(240, 180, 180)),
                                    );
                                });
                            ui.add_space(4.0);
                            ui.separator();
                        }
                        WorkerStatus::Ready => {}
                    }

                    ui.add_space(6.0);

                    // 1. Play Side / Perspective Selector
                    ui.label(
                        egui::RichText::new("PLAYING PERSPECTIVE")
                            .strong()
                            .size(11.0)
                            .color(egui::Color32::from_rgb(160, 175, 200)),
                    );
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
                    ui.label(
                        egui::RichText::new("Shortcut: Press 'B' key globally to switch side")
                            .italics()
                            .size(10.5)
                            .color(egui::Color32::from_rgb(130, 140, 160)),
                    );

                    ui.add_space(8.0);
                    ui.separator();
                    ui.add_space(6.0);

                    // 2. Engine Settings
                    ui.label(
                        egui::RichText::new("STOCKFISH ENGINE")
                            .strong()
                            .size(11.0)
                            .color(egui::Color32::from_rgb(160, 175, 200)),
                    );
                    ui.add(egui::Slider::new(&mut c.stockfish_depth, 1..=30).text("Search Depth"));
                    ui.add(
                        egui::Slider::new(&mut c.stockfish_lines, 1..=5).text("Suggested Lines"),
                    );
                    ui.add(
                        egui::Slider::new(&mut c.stockfish_time_ms, 10..=2_000)
                            .text("Search Budget (ms)"),
                    );
                    ui.add(egui::Slider::new(&mut c.fps, 1..=30).text("Scan Rate (FPS)"));

                    ui.add_space(8.0);
                    ui.separator();
                    ui.add_space(6.0);

                    // 3. Vision & Overlay Styling
                    ui.label(
                        egui::RichText::new("VISION & OVERLAY")
                            .strong()
                            .size(11.0)
                            .color(egui::Color32::from_rgb(160, 175, 200)),
                    );
                    ui.add(
                        egui::Slider::new(&mut c.confidence_threshold, 0.1..=0.9)
                            .text("AI Confidence"),
                    );
                    ui.add(
                        egui::Slider::new(&mut c.arrow_thickness, 3.0..=12.0)
                            .text("Arrow Thickness"),
                    );
                    ui.checkbox(
                        &mut c.stealth_mode,
                        "Anti-Capture Stealth (Exclude from OBS/Share)",
                    );

                    ui.add_space(8.0);
                    ui.separator();
                    ui.add_space(8.0);

                    // 4. Region & Action Controls
                    let region_status = if let Some(r) = &c.board_region {
                        format!("Region: [{}, {}] {}x{}", r.x, r.y, r.width, r.height)
                    } else {
                        "Region: Not Selected".to_string()
                    };
                    ui.label(
                        egui::RichText::new(region_status)
                            .size(11.0)
                            .color(egui::Color32::from_rgb(150, 165, 185)),
                    );

                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        if ui.button("📐 Select Board (R)").clicked() {
                            self.selection_active.store(true, Ordering::SeqCst);
                        }

                        if ui.button("💾 Save Settings").clicked() && c.save().is_ok() {
                            self.save_feedback_timer = Some(Instant::now());
                            println!("Configuration saved to config.json");
                        }

                        if let Some(t) = self.save_feedback_timer {
                            if t.elapsed() < Duration::from_secs(2) {
                                ui.label(
                                    egui::RichText::new("✓ Saved")
                                        .color(egui::Color32::from_rgb(76, 217, 100))
                                        .strong(),
                                );
                            } else {
                                self.save_feedback_timer = None;
                            }
                        }
                    });

                    ui.add_space(8.0);
                    // Start / Stop Main Action Button
                    let is_ready = matches!(status, WorkerStatus::Ready);
                    let can_start = c.board_region.is_some() && is_ready;
                    if c.running {
                        let stop_btn = egui::Button::new(
                            egui::RichText::new("⏹ STOP ANALYSIS")
                                .size(14.0)
                                .strong()
                                .color(egui::Color32::WHITE),
                        )
                        .fill(egui::Color32::from_rgb(200, 40, 40))
                        .min_size(egui::vec2(ui.available_width(), 34.0));
                        if ui.add(stop_btn).clicked() {
                            c.running = false;
                        }
                    } else {
                        let start_btn = egui::Button::new(
                            egui::RichText::new("▶ START ANALYSIS")
                                .size(14.0)
                                .strong()
                                .color(egui::Color32::WHITE),
                        )
                        .fill(if can_start {
                            egui::Color32::from_rgb(34, 150, 75)
                        } else {
                            egui::Color32::from_rgb(60, 70, 80)
                        })
                        .min_size(egui::vec2(ui.available_width(), 34.0));
                        if ui.add_enabled(can_start, start_btn).clicked() {
                            c.running = true;
                        }

                        if !can_start {
                            let hint = if !is_ready {
                                "Cannot start: missing or initializing assets..."
                            } else {
                                "Select board region first (Press 'R')"
                            };
                            ui.add_space(3.0);
                            ui.label(
                                egui::RichText::new(hint)
                                    .italics()
                                    .size(10.5)
                                    .color(egui::Color32::from_rgb(200, 160, 100)),
                            );
                        }
                    }
                });

            if let Some(resp) = win_resp {
                self.control_panel_rect = Some(resp.response.rect);
            }
        } else {
            self.control_panel_rect = None;
        }

        // Central Transparent Canvas for Selection & Move Arrows
        egui::CentralPanel::default()
            .frame(egui::Frame::none())
            .show(ctx, |ui| {
                let painter = ui.painter();

                if is_selecting {
                    let play_as_black = lock_config(&self.config).play_as_black;
                    // Top banner instruction
                    painter.rect_filled(
                        egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(ui.max_rect().width(), 50.0)),
                        0.0,
                        egui::Color32::from_black_alpha(200),
                    );
                    let banner_text = if play_as_black {
                        "📐 CLICK AND DRAG ON YOUR CHESSBOARD (FROM CORNER H1 TO A8) - [ESC] to Cancel"
                    } else {
                        "📐 CLICK AND DRAG ON YOUR CHESSBOARD (FROM CORNER A8 TO H1) - [ESC] to Cancel"
                    };
                    painter.text(
                        egui::pos2(ui.max_rect().center().x, 25.0),
                        egui::Align2::CENTER_CENTER,
                        banner_text,
                        egui::FontId::proportional(20.0),
                        egui::Color32::from_rgb(255, 215, 0),
                    );

                    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
                        self.selection_active.store(false, Ordering::SeqCst);
                        self.drag_start = None;
                    }

                    let (primary_down, pointer_pos) = ctx.input(|i| (i.pointer.primary_down(), i.pointer.latest_pos()));

                    if primary_down {
                        if self.drag_start.is_none() {
                            self.drag_start = pointer_pos;
                        }
                        if let (Some(start), Some(curr)) = (self.drag_start, pointer_pos) {
                            let rect = egui::Rect::from_two_pos(start, curr);
                            if rect.width() > 3.0 && rect.height() > 3.0 {
                                // Semi-transparent green fill
                                painter.rect_filled(
                                    rect,
                                    0.0,
                                    egui::Color32::from_rgba_unmultiplied(0, 230, 118, 35),
                                );
                                // Thick glowing green border
                                painter.rect_stroke(
                                    rect,
                                    0.0,
                                    egui::Stroke::new(3.0, egui::Color32::from_rgb(0, 230, 118)),
                                );
                                // 8x8 Grid lines & cell coordinate labels
                                let cell_w = rect.width() / 8.0;
                                let cell_h = rect.height() / 8.0;

                                for i in 1..8 {
                                    let x = rect.min.x + i as f32 * cell_w;
                                    let y = rect.min.y + i as f32 * cell_h;
                                    painter.line_segment(
                                        [egui::pos2(x, rect.min.y), egui::pos2(x, rect.max.y)],
                                        egui::Stroke::new(1.5, egui::Color32::from_rgba_unmultiplied(0, 230, 118, 180)),
                                    );
                                    painter.line_segment(
                                        [egui::pos2(rect.min.x, y), egui::pos2(rect.max.x, y)],
                                        egui::Stroke::new(1.5, egui::Color32::from_rgba_unmultiplied(0, 230, 118, 180)),
                                    );
                                }

                                // Draw square coordinates in every cell (e.g. a8, b8 ... h1)
                                for col in 0..8 {
                                    for row in 0..8 {
                                        let (file_char, rank_num) = if play_as_black {
                                            (
                                                (b'h' - col as u8) as char,
                                                row + 1,
                                            )
                                        } else {
                                            (
                                                (b'a' + col as u8) as char,
                                                8 - row,
                                            )
                                        };
                                        let cell_center = egui::pos2(
                                            rect.min.x + (col as f32 + 0.5) * cell_w,
                                            rect.min.y + (row as f32 + 0.5) * cell_h,
                                        );
                                        painter.text(
                                            cell_center,
                                            egui::Align2::CENTER_CENTER,
                                            format!("{}{}", file_char, rank_num),
                                            egui::FontId::proportional(11.0),
                                            egui::Color32::from_rgba_unmultiplied(255, 255, 255, 120),
                                        );
                                    }
                                }

                                // Size info banner
                                let ppp = ctx.pixels_per_point();
                                let corner_info = if play_as_black {
                                    "Align h1 (top-left) to a8 (bottom-right)"
                                } else {
                                    "Align a8 (top-left) to h1 (bottom-right)"
                                };
                                let dim_text = format!("{:.0} × {:.0} px | {}", rect.width() * ppp, rect.height() * ppp, corner_info);
                                painter.text(
                                    egui::pos2(rect.min.x + 8.0, rect.min.y + 10.0),
                                    egui::Align2::LEFT_TOP,
                                    dim_text,
                                    egui::FontId::monospace(13.0),
                                    egui::Color32::from_rgb(255, 215, 0),
                                );
                            }
                        }
                    } else {
                        // Released mouse button
                        if let (Some(start), Some(curr)) = (self.drag_start.take(), pointer_pos) {
                            let rect = egui::Rect::from_two_pos(start, curr);
                            if rect.width() > 40.0 && rect.height() > 40.0 {
                                let ppp = ctx.pixels_per_point();
                                let win_origin = get_window_client_origin("♟ MoveOverlay");
                                let board_region = crate::overlay::window::egui_rect_to_board_region(rect, win_origin, ppp);
                                let mut c = lock_config(&self.config);
                                c.board_region = Some(board_region);
                                let _ = c.save();
                                println!("Board region successfully saved: {:?}", c.board_region);
                                self.selection_active.store(false, Ordering::SeqCst);
                            }
                        }
                        self.drag_start = None;
                    }
                } else {
                    // Live move arrows
                    let (region, play_as_black, thickness, running) = {
                        let c = lock_config(&self.config);
                        (
                            c.board_region.clone(),
                            c.play_as_black,
                            c.arrow_thickness,
                            c.running,
                        )
                    };

                    if running {
                        if let Some(region) = region {
                            let ppp = ctx.pixels_per_point();
                            let win_origin = get_window_client_origin("♟ MoveOverlay");
                            let rect = crate::overlay::window::board_region_to_egui_rect(&region, win_origin, ppp);

                            for (i, m) in self.current_moves.iter().enumerate() {
                                let color = match i {
                                    0 => egui::Color32::from_rgba_unmultiplied(0, 230, 118, 240),
                                    1 => egui::Color32::from_rgba_unmultiplied(255, 193, 7, 215),
                                    2 => egui::Color32::from_rgba_unmultiplied(0, 229, 255, 185),
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

        ctx.request_repaint_after(Duration::from_millis(20));
    }
}
