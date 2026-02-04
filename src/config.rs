use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct BoardRegion {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct AppConfig {
    pub board_region: Option<BoardRegion>,
    pub stockfish_depth: u32,
    pub stockfish_lines: u32,
    pub stockfish_time_ms: u32,
    pub confidence_threshold: f32,
    pub show_white_moves: bool,
    pub fps: u32,
    #[serde(default)]
    pub running: bool,
    #[serde(skip)]
    pub request_selection: bool,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            board_region: None,
            stockfish_depth: 15,
            stockfish_lines: 3,
            stockfish_time_ms: 500,
            confidence_threshold: 0.5,
            show_white_moves: true,
            fps: 3,
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
