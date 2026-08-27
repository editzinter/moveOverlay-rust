use anyhow::{anyhow, Result};
use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::time::{Duration, Instant};

pub struct Stockfish {
    child: Child,
    stdin: ChildStdin,
    reader: BufReader<ChildStdout>,
}

impl Stockfish {
    pub fn new(path: &str) -> Result<Self> {
        let mut child = Command::new(path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| anyhow!("Failed to capture stdin of Stockfish"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow!("Failed to capture stdout of Stockfish"))?;
        let reader = BufReader::new(stdout);

        let mut sf = Self {
            child,
            stdin,
            reader,
        };

        // Initial UCI handshake
        sf.send("uci")?;
        sf.wait_for("uciok", Duration::from_secs(5))?;

        // Optimal thread and hash allocation
        let num_cpus = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4);
        let threads = (num_cpus.saturating_sub(2)).clamp(1, 8);
        let _ = sf.set_option("Threads", &threads.to_string());
        let _ = sf.set_option("Hash", "128");

        println!("Stockfish initialized with {} threads", threads);
        Ok(sf)
    }

    pub fn set_option(&mut self, name: &str, value: &str) -> Result<()> {
        self.send(&format!("setoption name {} value {}", name, value))
    }

    pub fn analyze(&mut self, fen: &str, depth: u32, lines: u32) -> Result<Vec<String>> {
        // Sync engine state
        self.send("isready")?;
        self.wait_for("readyok", Duration::from_secs(2))?;

        let lines_clamped = lines.clamp(1, 5);
        self.set_option("MultiPV", &lines_clamped.to_string())?;
        self.send(&format!("position fen {}", fen))?;
        self.send(&format!("go depth {}", depth.clamp(1, 30)))?;

        let mut pv_map: BTreeMap<u32, String> = BTreeMap::new();
        let start_time = Instant::now();
        let timeout = Duration::from_millis(3500);

        loop {
            if start_time.elapsed() > timeout {
                let _ = self.send("stop");
                break;
            }

            let mut line = String::new();
            if self.reader.read_line(&mut line)? == 0 {
                break;
            }

            let line_str = line.trim();
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
        for i in 1..=lines_clamped {
            if let Some(m) = pv_map.get(&i) {
                result.push(m.clone());
            }
        }

        if result.is_empty() {
            result = pv_map.into_values().collect();
        }

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
            if start.elapsed() > timeout {
                return Err(anyhow!("Timed out waiting for {}", expected));
            }
            let mut line = String::new();
            if self.reader.read_line(&mut line)? == 0 {
                if start.elapsed() > Duration::from_millis(100) {
                    return Err(anyhow!("Engine stream closed while waiting for {}", expected));
                }
                continue;
            }
            if line.contains(expected) {
                return Ok(());
            }
        }
    }
}

impl Drop for Stockfish {
    fn drop(&mut self) {
        let _ = self.send("quit");
        let _ = self.child.kill();
    }
}
