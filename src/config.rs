use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct BoardRegion {
    pub x: i32,
    pub y: i32,
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
    #[serde(default)]
    pub stealth_mode: bool,
    #[serde(default = "default_window_title")]
    pub window_title: String,
    #[serde(default)]
    pub running: bool,
    #[serde(skip)]
    pub request_selection: bool,
}

fn default_stockfish_depth() -> u32 {
    13
}
fn default_stockfish_lines() -> u32 {
    2
}
fn default_stockfish_time_ms() -> u32 {
    500
}
fn default_confidence() -> f32 {
    0.5
}
fn default_fps() -> u32 {
    5
}
fn default_arrow_thickness() -> f32 {
    6.5
}
fn default_window_title() -> String {
    "Runtime Host".to_string()
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            board_region: None,
            stockfish_depth: 13,
            stockfish_lines: 2,
            stockfish_time_ms: 500,
            confidence_threshold: 0.5,
            play_as_black: false,
            fps: 5,
            arrow_thickness: 6.5,
            stealth_mode: false,
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
        Self::get_asset_path("config.json")
    }

    pub fn get_app_dir() -> PathBuf {
        if let Ok(exe_path) = std::env::current_exe() {
            if let Some(parent) = exe_path.parent() {
                if parent.join("best.onnx").exists() || parent.join("config.json").exists() {
                    return parent.to_path_buf();
                }
            }
        }
        std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
    }

    pub fn get_asset_path(filename: &str) -> PathBuf {
        let app_dir = Self::get_app_dir();
        let direct = app_dir.join(filename);
        if direct.exists() {
            return direct;
        }
        if let Ok(cwd) = std::env::current_dir() {
            let cwd_path = cwd.join(filename);
            if cwd_path.exists() {
                return cwd_path;
            }
        }
        direct
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config_values() {
        let cfg = AppConfig::default();
        assert_eq!(cfg.stockfish_depth, 13);
        assert_eq!(cfg.stockfish_lines, 2);
        assert_eq!(cfg.confidence_threshold, 0.5);
        assert_eq!(cfg.fps, 5);
        assert!(!cfg.play_as_black);
        assert!(!cfg.stealth_mode);
    }

    #[test]
    fn test_config_serde_roundtrip() {
        let cfg = AppConfig {
            board_region: Some(BoardRegion {
                x: -1920,
                y: -1080,
                width: 800,
                height: 800,
            }),
            play_as_black: true,
            stockfish_depth: 20,
            ..Default::default()
        };

        let json = serde_json::to_string(&cfg).unwrap();
        let loaded: AppConfig = serde_json::from_str(&json).unwrap();

        assert_eq!(loaded.board_region, cfg.board_region);
        assert!(loaded.play_as_black);
        assert_eq!(loaded.stockfish_depth, 20);
    }

    #[test]
    fn test_get_asset_path_resolution() {
        let p = AppConfig::get_asset_path("non_existent_test_file.xyz");
        assert!(p.to_string_lossy().contains("non_existent_test_file.xyz"));
    }
}
