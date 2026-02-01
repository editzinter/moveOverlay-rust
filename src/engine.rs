use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};

pub struct Stockfish {
    #[allow(dead_code)]
    process: Child,
    stdin: ChildStdin,
    reader: BufReader<std::process::ChildStdout>,
}

impl Stockfish {
    pub fn new(path: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let mut process = Command::new(path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped()) // Capture stderr
            .spawn()?;

        let stdin = process.stdin.take().ok_or("Failed to open stdin")?;
        let stdout = process.stdout.take().ok_or("Failed to open stdout")?;
        let reader = BufReader::new(stdout);

        let mut engine = Self {
            process,
            stdin,
            reader,
        };

        println!("[Stockfish] Initializing...");
        engine.send_command("uci")?;

        // Read until uciok
        loop {
            let mut line = String::new();
            let bytes = engine.reader.read_line(&mut line)?;
            if bytes == 0 {
                return Err("Stockfish closed stream during init (EOF)".into());
            }
            // println!("[Stockfish Init]: {}", line.trim());
            if line.trim() == "uciok" {
                break;
            }
        }

        engine.send_command("isready")?;
        loop {
            let mut line = String::new();
            let bytes = engine.reader.read_line(&mut line)?;
            if bytes == 0 {
                return Err("Stockfish closed stream during isready".into());
            }
            if line.trim() == "readyok" {
                break;
            }
        }

        println!("[Stockfish] Ready and sync!");
        Ok(engine)
    }

    pub fn send_command(&mut self, cmd: &str) -> Result<(), Box<dyn std::error::Error>> {
        if let Err(e) = writeln!(self.stdin, "{}", cmd) {
            return Err(format!("Failed to write to Stockfish: {}", e).into());
        }
        if let Err(e) = self.stdin.flush() {
            return Err(format!("Failed to flush Stockfish stdin: {}", e).into());
        }
        Ok(())
    }

    pub fn set_option(
        &mut self,
        name: &str,
        value: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.send_command(&format!("setoption name {} value {}", name, value))
    }

    pub fn get_top_moves(
        &mut self,
        fen: &str,
        depth: u32,
    ) -> Result<Vec<String>, Box<dyn std::error::Error>> {
        self.send_command(&format!("position fen {}", fen))?;
        self.send_command(&format!("go depth {}", depth))?;

        let mut top_moves: std::collections::HashMap<u32, String> =
            std::collections::HashMap::new();

        loop {
            let mut line = String::new();
            let bytes = self.reader.read_line(&mut line)?;
            if bytes == 0 {
                return Err("Engine process closed stream".into());
            }

            let trimmed = line.trim();
            // println!("[Stockfish Output]: {}", trimmed); // Commented out to reduce spam

            // Parse info lines for PV
            // Example: info depth 10 ... multipv 1 ... pv e2e4 ...
            if trimmed.starts_with("info")
                && trimmed.contains(" multipv ")
                && trimmed.contains(" pv ")
            {
                if let Some(multipv_idx) = get_token_value(trimmed, "multipv") {
                    if let Some(pv_move) = get_token_value_str(trimmed, "pv") {
                        if let Ok(idx) = multipv_idx.parse::<u32>() {
                            top_moves.insert(idx, pv_move.to_string());
                        }
                    }
                }
            }

            if trimmed.starts_with("bestmove") {
                if top_moves.is_empty() {
                    // Fallback if no multipv info was parsed (e.g. fast mate or 1 line)
                    let parts: Vec<&str> = trimmed.split_whitespace().collect();
                    if parts.len() >= 2 {
                        return Ok(vec![parts[1].to_string()]);
                    }
                }

                // Return collected moves sorted by multipv index
                let mut sorted_moves: Vec<String> = Vec::new();
                let mut indices: Vec<u32> = top_moves.keys().cloned().collect();
                indices.sort();

                for idx in indices {
                    if let Some(m) = top_moves.get(&idx) {
                        sorted_moves.push(m.clone());
                    }
                }

                return Ok(sorted_moves);
            }
        }
    }

    pub fn restart(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        println!("[Stockfish] Restarting engine...");
        // Kill old process if possible
        let _ = self.process.kill();

        // Spawn new one
        let mut process = Command::new("stockfish.exe")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        let stdin = process.stdin.take().ok_or("Failed to open stdin")?;
        let stdout = process.stdout.take().ok_or("Failed to open stdout")?;
        let reader = BufReader::new(stdout);

        self.process = process;
        self.stdin = stdin;
        self.reader = reader;

        // Handshake again
        self.send_command("uci")?;
        loop {
            let mut line = String::new();
            let bytes = self.reader.read_line(&mut line)?;
            if bytes == 0 {
                return Err("EOF during restart".into());
            }
            if line.trim() == "uciok" {
                break;
            }
        }
        self.send_command("isready")?;
        loop {
            let mut line = String::new();
            let bytes = self.reader.read_line(&mut line)?;
            if bytes == 0 {
                return Err("EOF during restart isready".into());
            }
            if line.trim() == "readyok" {
                break;
            }
        }

        println!("[Stockfish] Restarted successfully!");
        Ok(())
    }
}

fn get_token_value<'a>(line: &'a str, token: &str) -> Option<&'a str> {
    let mut parts = line.split_whitespace();
    while let Some(part) = parts.next() {
        if part == token {
            return parts.next();
        }
    }
    None
}

fn get_token_value_str<'a>(line: &'a str, token: &str) -> Option<&'a str> {
    get_token_value(line, token)
}
