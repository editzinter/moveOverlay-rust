use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

#[derive(Debug, Serialize, Deserialize, Clone, Copy)]
pub struct Region {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AppConfig {
    pub region: Option<Region>,
    pub stockfish: StockfishConfig,
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy)]
pub struct StockfishConfig {
    pub depth: u32,
    pub multipv: u32,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            region: None,
            stockfish: StockfishConfig {
                depth: 15,
                multipv: 1,
            },
        }
    }
}

pub fn load_config() -> AppConfig {
    if let Ok(content) = fs::read_to_string("config.json") {
        if let Ok(cfg) = serde_json::from_str(&content) {
            return cfg;
        }
    }
    // Fallback: try loading legacy region.json
    let mut cfg = AppConfig::default();
    if Path::new("region.json").exists() {
        if let Ok(content) = fs::read_to_string("region.json") {
            if let Ok(region) = serde_json::from_str(&content) {
                cfg.region = Some(region);
            }
        }
    }
    cfg
}

pub fn save_config(cfg: &AppConfig) -> Result<(), std::io::Error> {
    let content = serde_json::to_string_pretty(cfg)?;
    fs::write("config.json", content)
}
