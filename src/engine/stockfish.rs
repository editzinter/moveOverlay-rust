use anyhow::{anyhow, Result};
use crossbeam_channel::{unbounded, Receiver};
use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

pub struct Stockfish {
    child: Child,
    stdin: ChildStdin,
    line_rx: Receiver<String>,
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
        // Drain any stale output from previous commands
        while self.line_rx.try_recv().is_ok() {}

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
