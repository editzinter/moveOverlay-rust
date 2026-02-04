use anyhow::{anyhow, Result};
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

pub struct Stockfish {
    child: Child,
    path: String,
}

impl Stockfish {
    pub fn new(path: &str) -> Result<Self> {
        let child = Command::new(path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        let mut sf = Self {
            child,
            path: path.to_string(),
        };

        // Initial handshake
        sf.send("uci")?;
        sf.wait_for("uciok", Duration::from_secs(5))?;

        // Optimize for your 12-core i5-12500H CPU
        sf.set_option("Threads", "8")?;
        sf.set_option("Hash", "256")?;

        println!("Stockfish initialized successfully");
        Ok(sf)
    }

    pub fn set_option(&mut self, name: &str, value: &str) -> Result<()> {
        self.send(&format!("setoption name {} value {}", name, value))
    }

    pub fn stop(&mut self) -> Result<()> {
        self.send("stop")?;
        // Clear any pending output
        let mut reader = BufReader::new(self.child.stdout.as_mut().ok_or(anyhow!("No stdout"))?);
        let mut line = String::new();
        // We don't want to block here, so we just do a quick read if possible
        // Actually, "isready" is better to sync
        self.send("isready")?;
        self.wait_for("readyok", Duration::from_secs(2))?;
        Ok(())
    }

    pub fn analyze(&mut self, fen: &str, depth: u32, lines: u32) -> Result<Vec<String>> {
        // Sync engine
        self.send("isready")?;
        self.wait_for("readyok", Duration::from_secs(2))?;

        self.set_option("MultiPV", &lines.to_string())?;
        self.send(&format!("position fen {}", fen))?;
        self.send(&format!("go depth {}", depth))?;

        let mut moves = Vec::new();
        let start_time = Instant::now();
        let timeout = Duration::from_secs(5); // Maximum 5 seconds for any scan

        // Get a mutable reference to stdout
        let stdout = self
            .child
            .stdout
            .as_mut()
            .ok_or(anyhow!("Failed to open stdout"))?;
        let mut reader = BufReader::new(stdout);

        loop {
            if start_time.elapsed() > timeout {
                println!("WARNING: Stockfish analysis timed out!");
                // Force stop if it hangs
                let _ = self.send("stop");
                break;
            }

            let mut line = String::new();
            // Note: read_line is blocking. In a perfect world we'd use async or non-blocking
            // but for UCI depth-based search it usually responds fast.
            reader.read_line(&mut line)?;

            if line.is_empty() {
                break;
            }

            if line.contains("bestmove") {
                break;
            }

            if line.contains("info depth") && line.contains(" pv ") {
                // Parse the move
                if let Some(pv_part) = line.split(" pv ").nth(1) {
                    let best_move = pv_part.split_whitespace().next().unwrap_or("").to_string();
                    if !best_move.is_empty() && !moves.contains(&best_move) {
                        moves.push(best_move);
                    }
                }
            }
        }

        // Keep only the most recent N moves from the info lines (MultiPV)
        // PV lines come in order of PV 1, PV 2, etc. in the final depth
        Ok(moves.into_iter().rev().take(lines as usize).collect())
    }

    fn send(&mut self, msg: &str) -> Result<()> {
        let stdin = self
            .child
            .stdin
            .as_mut()
            .ok_or(anyhow!("Failed to open stdin"))?;
        writeln!(stdin, "{}", msg)?;
        stdin.flush()?;
        Ok(())
    }

    fn wait_for(&mut self, expected: &str, timeout: Duration) -> Result<()> {
        let start = Instant::now();
        let stdout = self.child.stdout.as_mut().ok_or(anyhow!("No stdout"))?;
        let mut reader = BufReader::new(stdout);

        loop {
            if start.elapsed() > timeout {
                return Err(anyhow!("Timed out waiting for {}", expected));
            }
            let mut line = String::new();
            reader.read_line(&mut line)?;
            if line.contains(expected) {
                return Ok(());
            }
            if line.is_empty() && start.elapsed() > Duration::from_millis(100) {
                // Process might have died
                return Err(anyhow!(
                    "Engine stream closed while waiting for {}",
                    expected
                ));
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
