use anyhow::{anyhow, Result};
use crossbeam_channel::{unbounded, Receiver};
use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use crate::config::PlayMode;

#[cfg(windows)]
use std::os::windows::process::CommandExt;

pub struct Stockfish {
    child: Child,
    stdin: ChildStdin,
    line_rx: Receiver<String>,
    current_mode: Option<PlayMode>,
}

impl Stockfish {
    pub fn new(path: &str) -> Result<Self> {
        let mut cmd = Command::new(path);
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());

        #[cfg(windows)]
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW

        let mut child = cmd.spawn()?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| anyhow!("Failed to capture stdin of Stockfish"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow!("Failed to capture stdout of Stockfish"))?;

        let (line_tx, line_rx) = unbounded::<String>();

        // Dedicated background reader thread to eliminate any stdout blocking
        thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            let mut line = String::new();
            while let Ok(n) = reader.read_line(&mut line) {
                if n == 0 {
                    break;
                }
                let trimmed = line.trim().to_string();
                line.clear();
                if line_tx.send(trimmed).is_err() {
                    break;
                }
            }
        });

        let mut sf = Self {
            child,
            stdin,
            line_rx,
            current_mode: None,
        };

        // Initial UCI handshake
        sf.send("uci")?;
        sf.wait_for("uciok", Duration::from_secs(5))?;

        // Optimal thread and hash allocation
        let num_cpus = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4);
        let threads = (num_cpus.saturating_sub(2)).clamp(1, 8);
        sf.set_option("Threads", &threads.to_string())?;
        sf.set_option("Hash", "128")?;
        // UCI options are applied asynchronously by some engines. Ensure they are
        // active before the first timed search is issued.
        sf.send("isready")?;
        sf.wait_for("readyok", Duration::from_secs(2))?;

        println!("Stockfish initialized with {} threads", threads);
        Ok(sf)
    }

    pub fn set_option(&mut self, name: &str, value: &str) -> Result<()> {
        self.send(&format!("setoption name {} value {}", name, value))
    }

    pub fn apply_mode(&mut self, mode: PlayMode) -> Result<()> {
        if self.current_mode == Some(mode) {
            return Ok(());
        }

        match mode {
            PlayMode::Engine | PlayMode::Aggressive | PlayMode::Book => {
                self.set_option("UCI_LimitStrength", "false")?;
                self.set_option("Skill Level", "20")?;
            }
            PlayMode::Human => {
                self.set_option("UCI_LimitStrength", "true")?;
                self.set_option("UCI_Elo", "1950")?;
            }
        }

        self.send("isready")?;
        self.wait_for("readyok", Duration::from_secs(2))?;
        self.current_mode = Some(mode);
        Ok(())
    }

    pub fn analyze(
        &mut self,
        fen: &str,
        depth: u32,
        lines: u32,
        time_limit_ms: u32,
        mode: PlayMode,
    ) -> Result<Vec<String>> {
        // Book Mode: instant theoretical lookup if position exists in opening database
        if mode == PlayMode::Book {
            if let Some(book_moves) = crate::engine::book::get_book_moves(fen) {
                if !book_moves.is_empty() {
                    let take_count = (lines.clamp(1, 5) as usize).min(book_moves.len());
                    return Ok(book_moves[..take_count].to_vec());
                }
            }
            // If out of book, smoothly fall back to Stockfish calculation below
        }

        self.apply_mode(mode)?;

        // Drain any stale output from previous commands
        while self.line_rx.try_recv().is_ok() {}

        let lines_clamped = lines.clamp(1, 5);
        let search_multipv = if mode == PlayMode::Aggressive {
            lines_clamped.max(4)
        } else {
            lines_clamped
        };
        self.set_option("MultiPV", &search_multipv.to_string())?;
        self.send(&format!("position fen {}", fen))?;
        // A depth search has unbounded wall-clock time: tactical positions can
        // take orders of magnitude longer than quiet ones. Use a time budget so
        // the screen-to-overlay latency remains predictable. Keep `depth` as a
        // ceiling to preserve the UI's quality control.
        let time_limit_ms = time_limit_ms.clamp(10, 2_000);
        self.send(&format!(
            "go movetime {} depth {}",
            time_limit_ms,
            depth.clamp(1, 30)
        ))?;

        let mut pv_map: BTreeMap<u32, String> = BTreeMap::new();
        let start_time = Instant::now();
        // Allow a small grace period for the engine to flush its final PV and
        // bestmove after the requested move time.
        let timeout = Duration::from_millis(u64::from(time_limit_ms) + 250);

        loop {
            let elapsed = start_time.elapsed();
            if elapsed >= timeout {
                let _ = self.send("stop");
                // Wait briefly for the bestmove response after stopping
                let stop_deadline = Instant::now() + Duration::from_millis(300);
                while Instant::now() < stop_deadline {
                    if let Ok(line_str) = self.line_rx.recv_timeout(Duration::from_millis(50)) {
                        if line_str.starts_with("bestmove") {
                            break;
                        }
                    } else {
                        break;
                    }
                }
                break;
            }

            let remaining = timeout - elapsed;
            let line_str = match self
                .line_rx
                .recv_timeout(remaining.min(Duration::from_millis(100)))
            {
                Ok(l) => l,
                Err(crossbeam_channel::RecvTimeoutError::Timeout) => continue,
                Err(crossbeam_channel::RecvTimeoutError::Disconnected) => break,
            };

            if line_str.starts_with("bestmove") {
                if pv_map.is_empty() {
                    let parts: Vec<&str> = line_str.split_whitespace().collect();
                    if parts.len() >= 2 && parts[1] != "(none)" {
                        pv_map.insert(1, parts[1].to_string());
                    }
                }
                break;
            }

            if line_str.contains(" pv ") {
                let multipv_idx = if let Some(mpv_pos) = line_str.find(" multipv ") {
                    line_str[mpv_pos + 9..]
                        .split_whitespace()
                        .next()
                        .and_then(|s| s.parse::<u32>().ok())
                        .unwrap_or(1)
                } else {
                    1
                };

                if let Some(pv_part) = line_str.split(" pv ").nth(1) {
                    if let Some(first_move) = pv_part.split_whitespace().next() {
                        if first_move.len() >= 4 {
                            pv_map.insert(multipv_idx, first_move.to_string());
                        }
                    }
                }
            }
        }

        let mut result = Vec::new();
        for i in 1..=search_multipv {
            if let Some(m) = pv_map.get(&i) {
                result.push(m.clone());
            }
        }

        if result.is_empty() {
            result = pv_map.into_values().collect();
        }

        if mode == PlayMode::Aggressive {
            result = crate::vision::board::prioritize_aggressive_moves(fen, &result);
        }

        result.truncate(lines_clamped as usize);
        Ok(result)
    }

    fn send(&mut self, msg: &str) -> Result<()> {
        writeln!(self.stdin, "{}", msg)?;
        self.stdin.flush()?;
        Ok(())
    }

    fn wait_for(&mut self, expected: &str, timeout: Duration) -> Result<()> {
        let start = Instant::now();
        loop {
            let elapsed = start.elapsed();
            if elapsed > timeout {
                return Err(anyhow!("Timed out waiting for {}", expected));
            }
            let remaining = timeout - elapsed;
            match self
                .line_rx
                .recv_timeout(remaining.min(Duration::from_millis(200)))
            {
                Ok(line) => {
                    if line.contains(expected) {
                        return Ok(());
                    }
                }
                Err(crossbeam_channel::RecvTimeoutError::Timeout) => continue,
                Err(crossbeam_channel::RecvTimeoutError::Disconnected) => {
                    return Err(anyhow!(
                        "Engine stream closed while waiting for {}",
                        expected
                    ));
                }
            }
        }
    }
}

impl Drop for Stockfish {
    fn drop(&mut self) {
        let _ = self.send("quit");
        thread::sleep(Duration::from_millis(50));
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stockfish_modes_and_book() {
        let exe_path = crate::config::AppConfig::get_asset_path("stockfish.exe");
        if !exe_path.exists() {
            return;
        }

        let mut sf = Stockfish::new(exe_path.to_str().unwrap()).expect("Stockfish should initialize");
        let start_fen = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";

        // Book mode returns book moves directly
        let book_moves = sf
            .analyze(start_fen, 10, 2, 100, PlayMode::Book)
            .expect("Book analysis should succeed");
        assert!(!book_moves.is_empty());
        assert!(book_moves.contains(&"e2e4".to_string()) || book_moves.contains(&"d2d4".to_string()));

        // Human mode
        let human_moves = sf
            .analyze(start_fen, 10, 1, 100, PlayMode::Human)
            .expect("Human analysis should succeed");
        assert_eq!(human_moves.len(), 1);

        // Engine mode
        let engine_moves = sf
            .analyze(start_fen, 10, 2, 100, PlayMode::Engine)
            .expect("Engine analysis should succeed");
        assert_eq!(engine_moves.len(), 2);
    }
}

