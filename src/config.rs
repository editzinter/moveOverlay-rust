use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct BoardRegion {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct AppConfig {
    pub board_region: Option<BoardRegion>,
    #[serde(default = "default_stockfish_depth")]
    pub stockfish_depth: u32,
    #[serde(default = "default_stockfish_lines")]
    pub stockfish_lines: u32,
    #[serde(default = "default_stockfish_time_ms")]
    pub stockfish_time_ms: u32,
    #[serde(default = "default_confidence")]
    pub confidence_threshold: f32,
    #[serde(default)]
    pub play_as_black: bool,
    #[serde(default = "default_fps")]
    pub fps: u32,
    #[serde(default = "default_arrow_thickness")]
    pub arrow_thickness: f32,
    #[serde(default = "default_true")]
    pub stealth_mode: bool,
    #[serde(default = "default_window_title")]
    pub window_title: String,
    #[serde(default)]
    pub running: bool,
    #[serde(skip)]
    pub request_selection: bool,
}

fn default_stockfish_depth() -> u32 {
    15
}
fn default_stockfish_lines() -> u32 {
    3
}
fn default_stockfish_time_ms() -> u32 {
    500
}
fn default_confidence() -> f32 {
    0.5
}
fn default_fps() -> u32 {
    3
}
fn default_arrow_thickness() -> f32 {
    6.5
}
fn default_true() -> bool {
    true
}
fn default_window_title() -> String {
    "Runtime Host".to_string()
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            board_region: None,
            stockfish_depth: 15,
            stockfish_lines: 3,
            stockfish_time_ms: 500,
            confidence_threshold: 0.5,
            play_as_black: false,
            fps: 3,
            arrow_thickness: 6.5,
            stealth_mode: true,
            window_title: "Runtime Host".to_string(),
            running: false,
            request_selection: false,
        }
    }
}

impl AppConfig {
    pub fn load() -> Self {
        let path = Self::config_path();
        if let Ok(content) = fs::read_to_string(path) {
            if let Ok(config) = serde_json::from_str(&content) {
                return config;
            }
        }
        Self::default()
    }

    pub fn save(&self) -> anyhow::Result<()> {
        let path = Self::config_path();
        let content = serde_json::to_string_pretty(self)?;
        fs::write(path, content)?;
        Ok(())
    }

    fn config_path() -> PathBuf {
        PathBuf::from("config.json")
    }
}
