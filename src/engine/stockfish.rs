use anyhow::{anyhow, Result};
use crossbeam_channel::{unbounded, Receiver};
use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use crate::config::PlayMode;

// Stockfish can stop halfway through a MultiPV depth. Endurance only compares
// candidates whose exact scores came from the same completed search depth.
type EndurancePvs = BTreeMap<u32, BTreeMap<u32, (String, i32)>>;

fn parse_endurance_pv(line: &str) -> Option<(u32, u32, String, i32)> {
    let tokens: Vec<_> = line.split_whitespace().collect();
    if tokens.first() != Some(&"info")
        || tokens.contains(&"lowerbound")
        || tokens.contains(&"upperbound")
    {
        return None;
    }
    let value_after = |key: &str| tokens.iter().position(|token| *token == key)
        .and_then(|i| tokens.get(i + 1).copied());
    let depth = value_after("depth")?.parse::<u32>().ok()?;
    let multipv = value_after("multipv").unwrap_or("1").parse::<u32>().ok()?;
    let score_index = tokens.iter().position(|token| *token == "score")?;
    let raw = tokens.get(score_index + 2)?.parse::<i32>().ok()?;
    let score = match *tokens.get(score_index + 1)? {
        "cp" => raw.clamp(-80_000, 80_000),
        "mate" if raw > 0 => 100_000 - raw.min(999),
        "mate" if raw < 0 => -100_000 + raw.saturating_abs().min(999),
        _ => return None,
    };
    let first_move = value_after("pv")?;
    if first_move.len() < 4 {
        return None;
    }
    Some((depth, multipv, first_move.to_string(), score))
}

fn complete_endurance_pvs(pvs: &EndurancePvs, expected: u32) -> Option<Vec<(String, i32)>> {
    pvs.iter().rev().find_map(|(depth, lines)| {
        (*depth >= 8 && expected > 0)
            .then(|| (1..=expected).map(|i| lines.get(&i).cloned()).collect::<Option<Vec<_>>>())
            .flatten()
    })
}

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
            PlayMode::Engine | PlayMode::Aggressive | PlayMode::Book | PlayMode::Gambit | PlayMode::Endurance => {
                self.set_option("UCI_LimitStrength", "false")?;
                self.set_option("Skill Level", "20")?;
            }
            PlayMode::Human => {
                self.set_option("UCI_LimitStrength", "true")?;
                self.set_option("UCI_Elo", "1800")?;
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
        let search_multipv = if mode == PlayMode::Endurance {
            // Favor enough depth to check safety before considering style.
            if time_limit_ms < 300 { 4 }
            else if time_limit_ms < 700 { 6 }
            else if time_limit_ms < 1_500 { 8 }
            else { 12 }
        } else if mode == PlayMode::Gambit {
            // MultiPV shares the same time budget across all candidates. At short
            // budgets, searching 12 lines leaves each evaluation too shallow
            // to judge whether a stylistic alternative is sound.
            if time_limit_ms < 300 {
                6
            } else if time_limit_ms < 700 {
                8
            } else {
                12
            }
        } else if mode == PlayMode::Aggressive {
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
        let mut evaluations = BTreeMap::new();
        let mut endurance_pvs = EndurancePvs::new();
        let mut best_move: Option<String> = None;
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
                            let parts: Vec<&str> = line_str.split_whitespace().collect();
                            if parts.len() >= 2 && parts[1] != "(none)" {
                                best_move = Some(parts[1].to_string());
                            }
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
                let parts: Vec<&str> = line_str.split_whitespace().collect();
                if parts.len() >= 2 && parts[1] != "(none)" {
                    best_move = Some(parts[1].to_string());
                }
                break;
            }

            if mode == PlayMode::Endurance {
                if let Some((depth, multipv, first_move, score)) = parse_endurance_pv(&line_str) {
                    endurance_pvs.entry(depth).or_default().insert(multipv, (first_move, score));
                }
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
                            let tokens: Vec<_> = line_str.split_whitespace().collect();
                            if let Some(i) = tokens.iter().position(|s| *s == "score") {
                                if let (Some(kind), Some(value)) = (tokens.get(i + 1), tokens.get(i + 2)) {
                                    if let Ok(n) = value.parse::<i32>() {
                                        let score = match *kind {
                                            "cp" => Some(n.clamp(-80_000, 80_000)),
                                            "mate" => Some(if n > 0 { 100_000 - n.min(999) } else { -100_000 + n.saturating_abs().min(999) }),
                                            _ => None,
                                        };
                                        if let Some(score) = score { evaluations.insert(first_move.to_string(), score); }
                                    }
                                }
                            }
                            pv_map.insert(multipv_idx, first_move.to_string());
                        }
                    }
                }
            }
        }

        let mut result = Vec::new();

        if mode == PlayMode::Human {
            // In Human mode, Stockfish's calibrated Elo decision is emitted in `bestmove`
            if let Some(ref bm) = best_move {
                result.push(bm.clone());
            }
            for i in 1..=search_multipv {
                if let Some(m) = pv_map.get(&i) {
                    if !result.contains(m) {
                        result.push(m.clone());
                    }
                }
            }
        } else if matches!(mode, PlayMode::Aggressive | PlayMode::Gambit | PlayMode::Endurance) {
            for i in 1..=search_multipv {
                if let Some(m) = pv_map.get(&i) {
                    result.push(m.clone());
                }
            }
            if result.is_empty() {
                if let Some(ref bm) = best_move {
                    result.push(bm.clone());
                }
            }
            result = match mode {
                PlayMode::Gambit => crate::engine::gambit::rank(fen, &result, &evaluations),
                PlayMode::Endurance => {
                    use shakmaty::{fen::Fen, CastlingMode, Chess, Position};
                    let legal_count = fen.parse::<Fen>().ok()
                        .and_then(|f| f.into_position::<Chess>(CastlingMode::Standard).ok())
                        .map(|p| p.legal_moves().len() as u32)
                        .unwrap_or(search_multipv);
                    let expected = search_multipv.min(legal_count);
                    if let Some(snapshot) = complete_endurance_pvs(&endurance_pvs, expected) {
                        let candidates: Vec<String> = snapshot.iter().map(|(m, _)| m.clone()).collect();
                        if best_move.as_ref().is_some_and(|bm| !candidates.contains(bm)) {
                            // A later, partial depth found a new best move. Trust it.
                            best_move.clone().into_iter().collect()
                        } else {
                            let scores = snapshot.into_iter().collect();
                            crate::engine::endurance::rank(fen, &candidates, &scores)
                        }
                    } else {
                        // No comparable candidate scores: use Stockfish's best move.
                        best_move.clone().into_iter().chain(result.into_iter().take(1)).take(1).collect()
                    }
                }
                _ => crate::vision::board::prioritize_aggressive_moves(fen, &result),
            };
        } else {
            // Engine mode
            for i in 1..=lines_clamped {
                if let Some(m) = pv_map.get(&i) {
                    result.push(m.clone());
                }
            }
            if result.is_empty() {
                if let Some(ref bm) = best_move {
                    result.push(bm.clone());
                }
            }
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
    fn endurance_uses_only_exact_scores_from_completed_depth() {
        let mut pvs = EndurancePvs::new();
        for info in [
            "info depth 8 multipv 1 score cp 25 pv e2e4 e7e5",
            "info depth 8 multipv 2 score cp 15 pv d2d4 d7d5",
            "info depth 9 multipv 1 score cp 40 pv g1f3 d7d5",
            "info depth 9 multipv 2 score cp 5 lowerbound pv c2c4 e7e5",
        ] {
            if let Some((depth, multipv, m, score)) = parse_endurance_pv(info) {
                pvs.entry(depth).or_default().insert(multipv, (m, score));
            }
        }
        assert_eq!(complete_endurance_pvs(&pvs, 2), Some(vec![("e2e4".into(), 25), ("d2d4".into(), 15)]));
        assert!(complete_endurance_pvs(&pvs, 3).is_none());
    }

    #[test]
    fn shallow_or_bounded_endurance_scores_are_not_used() {
        assert!(parse_endurance_pv("info depth 9 multipv 1 score cp 40 upperbound pv e2e4").is_none());
        let mut pvs = EndurancePvs::new();
        pvs.entry(7).or_default().insert(1, ("e2e4".into(), 20));
        assert!(complete_endurance_pvs(&pvs, 1).is_none());
    }

    #[test]
    #[ignore = "requires stockfish.exe; run cargo test -- --include-ignored"]
    fn test_stockfish_modes_and_book() {
        let exe_path = crate::config::AppConfig::get_asset_path("stockfish.exe");
        assert!(exe_path.exists(), "stockfish.exe is required for this integration test");

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

        // Exercise repeated switches on the same engine and unchanged position.
        for mode in [PlayMode::Aggressive, PlayMode::Gambit, PlayMode::Endurance, PlayMode::Human, PlayMode::Engine, PlayMode::Book] {
            let moves = sf.analyze(start_fen, 10, 2, 100, mode).unwrap();
            assert!(!moves.is_empty(), "{:?} returned no moves", mode);
            assert_eq!(crate::vision::board::validate_moves_for_side(start_fen, &moves, false), moves);
        }

        // Book misses must really invoke Stockfish, including after Human mode.
        sf.apply_mode(PlayMode::Human).unwrap();
        let endgame = "8/8/8/4k3/8/8/4K3/8 w - - 0 50";
        let moves = sf.analyze(endgame, 10, 2, 100, PlayMode::Book).unwrap();
        assert!(!moves.is_empty());
        assert_eq!(sf.current_mode, Some(PlayMode::Book));
        assert_eq!(crate::vision::board::validate_moves_for_side(endgame, &moves, false), moves);
    }
}

