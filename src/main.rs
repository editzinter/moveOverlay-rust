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
    Ready { detector_backend: &'static str },
    Recovering(String),
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

#[cfg(target_os = "windows")]
fn reassert_overlay_topmost(window_title: &str) {
    use windows::core::HSTRING;
    use windows::Win32::UI::WindowsAndMessaging::{
        FindWindowW, SetWindowPos, HWND_TOPMOST, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
    };
    unsafe {
        let title = HSTRING::from(window_title);
        if let Ok(hwnd) = FindWindowW(None, windows::core::PCWSTR(title.as_ptr())) {
            if !hwnd.is_invalid() {
                let _ = SetWindowPos(
                    hwnd,
                    HWND_TOPMOST,
                    0, 0, 0, 0,
                    SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE,
                );
            }
        }
    }
}

#[cfg(not(target_os = "windows"))]
fn reassert_overlay_topmost(_window_title: &str) {}

fn set_worker_status(status: &Arc<Mutex<WorkerStatus>>, value: WorkerStatus) {
    *status.lock().unwrap_or_else(|p| p.into_inner()) = value;
}

const ANALYSIS_REFRESH_INTERVAL: Duration = Duration::from_secs(30);
const ANALYSIS_RETRY_INTERVAL: Duration = Duration::from_secs(2);

fn analysis_due(
    fen: &str,
    last_success_fen: Option<&str>,
    last_success_at: Option<Instant>,
    last_attempt_fen: Option<&str>,
    last_attempt_at: Option<Instant>,
    now: Instant,
) -> bool {
    let still_fresh = last_success_fen == Some(fen)
        && last_success_at.is_some_and(|at| now.saturating_duration_since(at) < ANALYSIS_REFRESH_INTERVAL);
    let retry_cooling_down = last_attempt_fen == Some(fen)
        && last_attempt_at.is_some_and(|at| now.saturating_duration_since(at) < ANALYSIS_RETRY_INTERVAL);
    !still_fresh && !retry_cooling_down
}

fn valid_analysis_result(fen: &str, moves: &[String]) -> bool {
    if !moves.is_empty() {
        return true;
    }
    use shakmaty::{fen::Fen, CastlingMode, Chess, Position};
    fen.parse::<Fen>().ok()
        .and_then(|f| f.into_position::<Chess>(CastlingMode::Standard).ok())
        .is_some_and(|pos| pos.legal_moves().is_empty())
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
                    *ws = WorkerStatus::Ready { detector_backend: d.backend() };
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
        let model_path = AppConfig::get_asset_path("best.onnx");

        println!("Vision & Engine worker thread running.");

        let mut tracker = crate::vision::board::GameStateTracker::new();
        let mut last_analyzed_fen: Option<String> = None;
        let mut last_running_state = false;
        let mut last_play_side = false;
        let mut last_depth = 0;
        let mut last_lines = 0;
        let mut last_time_limit_ms = 0;
        let mut last_conf = -1.0f32;
        let mut last_region: Option<crate::config::BoardRegion> = None;
        let mut last_play_mode = crate::config::PlayMode::Engine;
        let mut invalid_detection_count: u32 = 0;
        let mut capture_error_count: u32 = 0;
        let mut detector_error_count: u32 = 0;
        let mut empty_analysis_count: u32 = 0;
        let mut last_detector_restart: Option<Instant> = None;
        let mut last_frame: Option<(u32, u32, Vec<u8>)> = None;
        let mut cached_fen: Option<String> = None;
        let mut last_success_at: Option<Instant> = None;
        let mut last_attempt_fen: Option<String> = None;
        let mut last_attempt_at: Option<Instant> = None;

        loop {
            let loop_start = std::time::Instant::now();
            let (region, depth, lines, time_limit_ms, conf, play_as_black, fps, running, play_mode) = {
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
                    c.play_mode,
                )
            };

            let board_source_changed = running != last_running_state
                || play_as_black != last_play_side
                || region != last_region;
            let analysis_changed = play_mode != last_play_mode
                || depth != last_depth
                || lines != last_lines
                || time_limit_ms != last_time_limit_ms
                || conf != last_conf;

            if board_source_changed {
                tracker.reset();
                invalid_detection_count = 0;
                capture_error_count = 0;
                detector_error_count = 0;
                empty_analysis_count = 0;
                last_frame = None;
                cached_fen = None;
            }
            if board_source_changed || analysis_changed {
                last_analyzed_fen = None;
                last_success_at = None;
                last_attempt_fen = None;
                last_attempt_at = None;
                last_running_state = running;
                last_play_side = play_as_black;
                last_depth = depth;
                last_lines = lines;
                last_time_limit_ms = time_limit_ms;
                last_conf = conf;
                last_region = region.clone();
                last_play_mode = play_mode;
                let _ = move_tx.send(Vec::new());
            }

            if running {
                if let Some(ref r) = region {
                    if r.width > 0 && r.height > 0 {
                        let capture_result = capture_region(r.x, r.y, r.width, r.height);

                        match capture_result {
                            Ok(img) => {
                                capture_error_count = 0;
                                let pixels = img.as_bytes();
                                let frame_unchanged = last_frame.as_ref().is_some_and(|(w, h, bytes)| {
                                    *w == img.width() && *h == img.height() && bytes == pixels
                                });
                                if !frame_unchanged {
                                    last_frame = Some((img.width(), img.height(), pixels.to_vec()));
                                    cached_fen = None;
                                }

                                // Reuse a settled board on identical frames, but still retry
                                // failed searches and refresh old suggestions periodically.
                                let fen = if frame_unchanged { cached_fen.clone() } else { None };
                                let fen = if fen.is_some() {
                                    fen
                                } else {
                                    match detector.detect(&img, conf) {
                                        Ok(detections) => {
                                            detector_error_count = 0;
                                            if let Some(board) = crate::vision::board::detections_to_board(&detections, play_as_black) {
                                                let settled = tracker.update(board, play_as_black);
                                                if let Some(ref valid_fen) = settled {
                                                    invalid_detection_count = 0;
                                                    cached_fen = Some(valid_fen.clone());
                                                } else if tracker.candidate_count >= 2 {
                                                    invalid_detection_count = invalid_detection_count.saturating_add(1);
                                                    if invalid_detection_count == 3 {
                                                        eprintln!("Stable board detection is not a legal chess position");
                                                        let _ = move_tx.send(Vec::new());
                                                        last_analyzed_fen = None;
                                                        last_success_at = None;
                                                        cached_fen = None;
                                                        tracker.reset();
                                                        set_worker_status(&worker_status_clone, WorkerStatus::Recovering("Detected board is not a legal position".into()));
                                                    }
                                                }
                                                settled
                                            } else {
                                                invalid_detection_count = invalid_detection_count.saturating_add(1);
                                                if invalid_detection_count == 3 {
                                                    eprintln!("Board detection invalid for three frames; clearing suggestions");
                                                    let _ = move_tx.send(Vec::new());
                                                    last_analyzed_fen = None;
                                                    last_success_at = None;
                                                    cached_fen = None;
                                                    tracker.reset();
                                                    set_worker_status(&worker_status_clone, WorkerStatus::Recovering("Board detection is unstable".into()));
                                                }
                                                None
                                            }
                                        }
                                        Err(e) => {
                                            detector_error_count = detector_error_count.saturating_add(1);
                                            if detector_error_count == 1 || detector_error_count == 3 {
                                                eprintln!("Vision inference error: {e}");
                                            }
                                            if detector_error_count >= 3 {
                                                if detector_error_count == 3 {
                                                    let _ = move_tx.send(Vec::new());
                                                    last_analyzed_fen = None;
                                                    last_success_at = None;
                                                    cached_fen = None;
                                                    tracker.reset();
                                                    set_worker_status(&worker_status_clone, WorkerStatus::Recovering("Vision inference failed; restarting detector".into()));
                                                }
                                                if last_detector_restart.is_none_or(|at| at.elapsed() >= Duration::from_secs(10)) {
                                                    last_detector_restart = Some(Instant::now());
                                                    match Detector::new(model_path.to_str().unwrap_or("best.onnx")) {
                                                        Ok(new_detector) => {
                                                            detector = new_detector;
                                                            detector_error_count = 0;
                                                        }
                                                        Err(restart_error) => eprintln!("Detector restart failed: {restart_error}"),
                                                    }
                                                }
                                            }
                                            None
                                        }
                                    }
                                };

                                if let Some(fen) = fen {
                                    let now = Instant::now();
                                    if analysis_due(&fen, last_analyzed_fen.as_deref(), last_success_at,
                                        last_attempt_fen.as_deref(), last_attempt_at, now) {
                                        if last_analyzed_fen.as_deref() != Some(&fen)
                                            && last_attempt_fen.as_deref() != Some(&fen) {
                                            let _ = move_tx.send(Vec::new());
                                        }
                                        last_attempt_fen = Some(fen.clone());
                                        last_attempt_at = Some(now);
                                        match sf.analyze(&fen, depth, lines, time_limit_ms, play_mode) {
                                            Ok(raw_moves) => {
                                                // A click during the blocking search invalidates its result.
                                                let current = lock_config(&config_clone);
                                                if current.play_mode != play_mode
                                                    || current.stockfish_depth != depth
                                                    || current.stockfish_lines != lines
                                                    || current.stockfish_time_ms != time_limit_ms
                                                    || current.confidence_threshold != conf
                                                    || current.play_as_black != play_as_black
                                                    || current.board_region != region
                                                    || !current.running
                                                {
                                                    continue;
                                                }
                                                drop(current);
                                                let valid_moves = crate::vision::board::validate_moves_for_side(&fen, &raw_moves, play_as_black);
                                                let no_nonmating_move = play_mode == crate::config::PlayMode::Endurance
                                                    && valid_moves.is_empty()
                                                    && crate::engine::endurance::fallback_nonmating_moves(&fen).is_empty();
                                                if valid_analysis_result(&fen, &valid_moves) || no_nonmating_move {
                                                    println!("▶ [{:?}] Board FEN: {} | Best moves: {:?}", play_mode, fen, valid_moves);
                                                    let _ = move_tx.send(valid_moves);
                                                    last_analyzed_fen = Some(fen);
                                                    last_success_at = Some(Instant::now());
                                                    empty_analysis_count = 0;
                                                    set_worker_status(&worker_status_clone, WorkerStatus::Ready { detector_backend: detector.backend() });
                                                } else {
                                                    empty_analysis_count = empty_analysis_count.saturating_add(1);
                                                    eprintln!("No legal suggestion for nonterminal board; retrying (attempt {})", empty_analysis_count);
                                                    set_worker_status(&worker_status_clone, WorkerStatus::Recovering("No legal engine suggestion; retrying".into()));
                                                    if empty_analysis_count >= 2 {
                                                        match Stockfish::new(engine_path.to_str().unwrap_or("stockfish.exe")) {
                                                            Ok(new_sf) => { sf = new_sf; empty_analysis_count = 0; }
                                                            Err(e) => eprintln!("Stockfish restart failed: {e}"),
                                                        }
                                                    }
                                                }
                                            }
                                            Err(e) => {
                                                eprintln!("Stockfish error: {e}. Restarting engine...");
                                                set_worker_status(&worker_status_clone, WorkerStatus::Recovering("Stockfish failed; restarting engine".into()));
                                                if let Ok(new_sf) = Stockfish::new(engine_path.to_str().unwrap_or("stockfish.exe")) {
                                                    sf = new_sf;
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                            Err(e) => {
                                capture_error_count = capture_error_count.saturating_add(1);
                                if capture_error_count == 1 || capture_error_count == 3 {
                                    eprintln!("Screen capture error: {e}");
                                }
                                if capture_error_count == 3 {
                                    let _ = move_tx.send(Vec::new());
                                    last_analyzed_fen = None;
                                    last_success_at = None;
                                    cached_fen = None;
                                    last_frame = None;
                                    tracker.reset();
                                    set_worker_status(&worker_status_clone, WorkerStatus::Recovering("Screen capture failed; retrying".into()));
                                }
                            }
                        }
                    }
                }
            }

            let scan_interval = Duration::from_secs_f64(1.0 / f64::from(fps.clamp(1, 30)));
            thread::sleep(scan_interval.saturating_sub(loop_start.elapsed()));
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
                last_z_order_refresh: Instant::now(),
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
    last_z_order_refresh: Instant,
}

impl eframe::App for OverlayWrapper {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 0.0]
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        while let Ok(moves) = self.move_rx.try_recv() {
            self.current_moves = moves;
        }

        if self.last_z_order_refresh.elapsed() >= Duration::from_secs(30) {
            if lock_config(&self.config).running {
                reassert_overlay_topmost("♟ MoveOverlay");
            }
            self.last_z_order_refresh = Instant::now();
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
                .default_size(egui::vec2(370.0, 580.0))
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
                        WorkerStatus::Recovering(reason) => {
                            ui.label(
                                egui::RichText::new(format!("Recovering: {reason}"))
                                    .size(10.5)
                                    .color(egui::Color32::from_rgb(255, 180, 90)),
                            );
                        }
                        WorkerStatus::Ready { detector_backend } => {
                            ui.label(
                                egui::RichText::new(format!("Vision: {}", detector_backend))
                                    .size(10.5)
                                    .color(egui::Color32::LIGHT_GRAY),
                            );
                        }
                    }

                    ui.add_space(6.0);

                    // 1. Play Mode Selector (Segmented buttons)
                    ui.label(
                        egui::RichText::new("ANALYSIS MODE")
                            .strong()
                            .size(11.0)
                            .color(egui::Color32::from_rgb(160, 175, 200)),
                    );
                    ui.horizontal_wrapped(|ui| {
                        let engine_btn = ui.selectable_label(c.play_mode == crate::config::PlayMode::Engine, "⚙ Engine");
                        if engine_btn.clicked() {
                            c.play_mode = crate::config::PlayMode::Engine;
                        }
                        let human_btn = ui.selectable_label(c.play_mode == crate::config::PlayMode::Human, "🧠 Human");
                        if human_btn.clicked() {
                            c.play_mode = crate::config::PlayMode::Human;
                        }
                        let book_btn = ui.selectable_label(c.play_mode == crate::config::PlayMode::Book, "📖 Book");
                        if book_btn.clicked() {
                            c.play_mode = crate::config::PlayMode::Book;
                        }
                        let agg_btn = ui.selectable_label(c.play_mode == crate::config::PlayMode::Aggressive, "⚔ Aggressive");
                        if agg_btn.clicked() {
                            c.play_mode = crate::config::PlayMode::Aggressive;
                        }
                        if ui.selectable_label(c.play_mode == crate::config::PlayMode::Gambit, "♟ Gambit").clicked() {
                            c.play_mode = crate::config::PlayMode::Gambit;
                        }
                        if ui.selectable_label(c.play_mode == crate::config::PlayMode::Endurance, "⌛ Endurance").clicked() {
                            c.play_mode = crate::config::PlayMode::Endurance;
                        }
                    });

                    let (mode_desc, desc_color) = match c.play_mode {
                        crate::config::PlayMode::Gambit => (
                            "Gambit: Prefers material offers only when Stockfish rates them near its best move.",
                            egui::Color32::from_rgb(210, 140, 255),
                        ),
                        crate::config::PlayMode::Endurance => (
                            "Endurance: Favors playable moves that keep the game going longer.",
                            egui::Color32::from_rgb(110, 220, 205),
                        ),
                        crate::config::PlayMode::Engine => (
                            "Engine: Superhuman Stockfish calculations (3500+ Elo).",
                            egui::Color32::from_rgb(100, 200, 255),
                        ),
                        crate::config::PlayMode::Human => (
                            "Human: Stockfish strength limited to a target of 1800 Elo.",
                            egui::Color32::from_rgb(160, 230, 130),
                        ),
                        crate::config::PlayMode::Book => (
                            "Book: Built-in opening moves with Stockfish fallback outside the book.",
                            egui::Color32::from_rgb(255, 210, 110),
                        ),
                        crate::config::PlayMode::Aggressive => (
                            "Aggressive: High-initiative sharp tactical moves (checks & captures).",
                            egui::Color32::from_rgb(255, 130, 130),
                        ),
                    };

                    ui.label(
                        egui::RichText::new(mode_desc)
                            .italics()
                            .size(10.5)
                            .color(desc_color),
                    );

                    ui.add_space(8.0);
                    ui.separator();
                    ui.add_space(6.0);

                    // 2. Play Side / Perspective Selector
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
                    ui.add(egui::Slider::new(&mut c.stockfish_depth, 1..=30).text("Depth Limit"));
                    ui.add(
                        egui::Slider::new(&mut c.stockfish_lines, 1..=5).text("Suggested Lines"),
                    );
                    ui.add(
                        egui::Slider::new(&mut c.stockfish_time_ms, 10..=2_000)
                            .text("Search Budget (ms)"),
                    );
                    ui.add(egui::Slider::new(&mut c.fps, 1..=30).text("Maximum Scan FPS"));

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

                    if !self.current_moves.is_empty() {
                        ui.add_space(4.0);
                        egui::Frame::none()
                            .fill(egui::Color32::from_rgb(25, 28, 36))
                            .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(50, 58, 72)))
                            .rounding(4.0)
                            .inner_margin(6.0)
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    let (badge_text, badge_color) = match c.play_mode {
                                        crate::config::PlayMode::Gambit => ("♟ GAMBIT", egui::Color32::from_rgb(190, 100, 255)),
                                        crate::config::PlayMode::Endurance => ("⌛ ENDURANCE", egui::Color32::from_rgb(80, 210, 190)),
                                        crate::config::PlayMode::Engine => ("⚙ ENGINE", egui::Color32::from_rgb(0, 230, 118)),
                                        crate::config::PlayMode::Human => ("🧠 HUMAN", egui::Color32::from_rgb(33, 150, 243)),
                                        crate::config::PlayMode::Book => ("📖 BOOK", egui::Color32::from_rgb(255, 215, 0)),
                                        crate::config::PlayMode::Aggressive => ("⚔ AGGRESSIVE", egui::Color32::from_rgb(255, 23, 68)),
                                    };
                                    ui.label(
                                        egui::RichText::new(badge_text)
                                            .strong()
                                            .size(11.0)
                                            .color(badge_color),
                                    );
                                    ui.label(
                                        egui::RichText::new(format!("Suggested: {}", self.current_moves.join("  →  ")))
                                            .strong()
                                            .size(11.5)
                                            .color(egui::Color32::WHITE),
                                    );
                                });
                            });
                    }

                    ui.add_space(8.0);
                    // Start / Stop Main Action Button
                    let is_ready = matches!(status, WorkerStatus::Ready { .. } | WorkerStatus::Recovering(_));
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
                    let (region, play_as_black, thickness, running, play_mode) = {
                        let c = lock_config(&self.config);
                        (
                            c.board_region.clone(),
                            c.play_as_black,
                            c.arrow_thickness,
                            c.running,
                            c.play_mode,
                        )
                    };

                    if running {
                        if let Some(region) = region {
                            let ppp = ctx.pixels_per_point();
                            let win_origin = get_window_client_origin("♟ MoveOverlay");
                            let rect = crate::overlay::window::board_region_to_egui_rect(&region, win_origin, ppp);

                            for (i, m) in self.current_moves.iter().enumerate() {
                                let color = match (play_mode, i) {
                                    (crate::config::PlayMode::Gambit, _) => egui::Color32::from_rgba_unmultiplied(190, 100, 255, 245u8.saturating_sub((i.min(4) as u8) * 25)),
                                    (crate::config::PlayMode::Endurance, _) => egui::Color32::from_rgba_unmultiplied(70, 220, 190, 245u8.saturating_sub((i.min(4) as u8) * 25)),
                                    // Engine mode: High-tech Emerald / Amber / Cyan
                                    (crate::config::PlayMode::Engine, 0) => egui::Color32::from_rgba_unmultiplied(0, 230, 118, 240),
                                    (crate::config::PlayMode::Engine, 1) => egui::Color32::from_rgba_unmultiplied(255, 193, 7, 215),
                                    (crate::config::PlayMode::Engine, 2) => egui::Color32::from_rgba_unmultiplied(0, 229, 255, 185),

                                    // Human mode: Distinct Sky Blue / Azure / Teal
                                    (crate::config::PlayMode::Human, 0) => egui::Color32::from_rgba_unmultiplied(33, 150, 243, 240),
                                    (crate::config::PlayMode::Human, 1) => egui::Color32::from_rgba_unmultiplied(79, 195, 247, 215),
                                    (crate::config::PlayMode::Human, 2) => egui::Color32::from_rgba_unmultiplied(129, 212, 250, 185),

                                    // Book mode: Grandmaster Theoretical Gold / Amber
                                    (crate::config::PlayMode::Book, 0) => egui::Color32::from_rgba_unmultiplied(255, 215, 0, 245),
                                    (crate::config::PlayMode::Book, 1) => egui::Color32::from_rgba_unmultiplied(255, 179, 0, 215),
                                    (crate::config::PlayMode::Book, 2) => egui::Color32::from_rgba_unmultiplied(255, 152, 0, 185),

                                    // Aggressive mode: Sharp Attack Crimson / Fiery Red-Orange
                                    (crate::config::PlayMode::Aggressive, 0) => egui::Color32::from_rgba_unmultiplied(255, 23, 68, 245),
                                    (crate::config::PlayMode::Aggressive, 1) => egui::Color32::from_rgba_unmultiplied(255, 87, 34, 220),
                                    (crate::config::PlayMode::Aggressive, 2) => egui::Color32::from_rgba_unmultiplied(255, 145, 0, 190),

                                    (_, _) => egui::Color32::from_rgba_unmultiplied(186, 104, 200, 140),
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

#[cfg(test)]
mod recovery_tests {
    use super::*;

    #[test]
    fn same_board_retries_after_failure_and_refreshes_after_success() {
        let now = Instant::now();
        let fen = "6k1/8/8/8/8/8/8/6K1 w - - 0 1";
        assert!(analysis_due(fen, None, None, None, None, now));
        assert!(!analysis_due(fen, None, None, Some(fen), Some(now), now + Duration::from_secs(1)));
        assert!(analysis_due("new position", None, None, Some(fen), Some(now), now + Duration::from_secs(1)));
        assert!(analysis_due(fen, None, None, Some(fen), Some(now), now + Duration::from_secs(3)));
        assert!(!analysis_due(fen, Some(fen), Some(now), Some(fen), Some(now), now + Duration::from_secs(10)));
        assert!(analysis_due(fen, Some(fen), Some(now), Some(fen), Some(now), now + Duration::from_secs(31)));
    }

    #[test]
    fn empty_result_is_only_final_for_a_terminal_position() {
        let playable = "6k1/8/8/8/8/8/8/6K1 w - - 0 1";
        let checkmate = "7k/6Q1/5K2/8/8/8/8/8 b - - 0 1";
        assert!(!valid_analysis_result(playable, &[]));
        assert!(valid_analysis_result(checkmate, &[]));
        assert!(!valid_analysis_result("invalid fen", &[]));
    }
}
