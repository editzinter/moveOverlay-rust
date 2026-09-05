// Opening Theory Database for Book Mode
// Maps normalized position FENs to theoretical Grandmaster book moves.

use std::collections::HashMap;
use std::sync::OnceLock;

static BOOK: OnceLock<HashMap<&'static str, &'static [&'static str]>> = OnceLock::new();

fn init_book() -> HashMap<&'static str, &'static [&'static str]> {
    let mut m = HashMap::new();

    // 1. Initial Position (White)
    m.insert(
        "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w",
        &["e2e4", "d2d4", "c2c4", "g1f3"][..],
    );

    // --- 1. e4 responses (Black) ---
    m.insert(
        "rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b",
        &["c7c5", "e7e5", "e7e6", "c7c6", "d7d6", "g8f6"][..],
    );

    // Open Game: 1. e4 e5
    m.insert(
        "rnbqkbnr/pppp1ppp/8/4p3/4P3/8/PPPP1PPP/RNBQKBNR w",
        &["g1f3", "f2f4", "b1c3", "d2d4"][..],
    );
    // 1. e4 e5 2. Nf3
    m.insert(
        "rnbqkbnr/pppp1ppp/8/4p3/4P3/5N2/PPPP1PPP/RNBQKB1R b",
        &["b8c6", "g8f6", "d7d6"][..],
    );
    // 1. e4 e5 2. Nf3 Nc6
    m.insert(
        "r1bqkbnr/pppp1ppp/2n5/4p3/4P3/5N2/PPPP1PPP/RNBQKB1R w",
        &["f1b5", "f1c4", "d2d4", "b1c3"][..],
    );
    // Ruy Lopez: 1. e4 e5 2. Nf3 Nc6 3. Bb5
    m.insert(
        "r1bqkbnr/pppp1ppp/2n5/1B2p3/4P3/5N2/PPPP1PPP/RNBQK2R b",
        &["a7a6", "g8f6", "d7d6", "f8c5"][..],
    );
    // Ruy Lopez: 3... a6 4. Ba4
    m.insert(
        "r1bqkbnr/1ppp1ppp/p1n5/1B2p3/4P3/5N2/PPPP1PPP/RNBQK2R w",
        &["b5a4", "b5xc6"][..],
    );
    // Italian Game: 1. e4 e5 2. Nf3 Nc6 3. Bc4
    m.insert(
        "r1bqkbnr/pppp1ppp/2n5/4p3/2B1P3/5N2/PPPP1PPP/RNBQK2R b",
        &["f8c5", "g8f6", "d7d6"][..],
    );
    // Scotch Game: 1. e4 e5 2. Nf3 Nc6 3. d4
    m.insert(
        "r1bqkbnr/pppp1ppp/2n5/4p3/3PP3/5N2/PPP2PPP/RNBQKB1R b",
        &["e5xd4"][..],
    );

    // Sicilian Defense: 1. e4 c5
    m.insert(
        "rnbqkbnr/pp1ppppp/8/2p5/4P3/8/PPPP1PPP/RNBQKBNR w",
        &["g1f3", "b1c3", "c2c3", "d2d4"][..],
    );
    // Sicilian: 2. Nf3
    m.insert(
        "rnbqkbnr/pp1ppppp/8/2p5/4P3/5N2/PPPP1PPP/RNBQKB1R b",
        &["d7d6", "b8c6", "e7e6", "g7g6"][..],
    );
    // Open Sicilian: 2. Nf3 d6 3. d4
    m.insert(
        "rnbqkbnr/pp2pppp/3p4/2p5/3PP3/5N2/PPP2PPP/RNBQKB1R b",
        &["c5xd4"][..],
    );
    // Open Sicilian: 3... cxd4 4. Nxd4
    m.insert(
        "rnbqkbnr/pp2pppp/3p4/8/3NP3/8/PPP2PPP/RNBQKB1R b",
        &["g8f6", "e7e6", "a7a6"][..],
    );
    // Open Sicilian: 4... Nf6 5. Nc3
    m.insert(
        "rnbqkb1r/pp2pppp/3p1n2/8/3NP3/2N5/PPP2PPP/R1BQKB1R b",
        &["a7a6", "g7g6", "e7e6", "b8c6"][..],
    );

    // French Defense: 1. e4 e6
    m.insert(
        "rnbqkbnr/pppp1ppp/4p3/8/4P3/8/PPPP1PPP/RNBQKBNR w",
        &["d2d4", "d2d3", "g1f3"][..],
    );
    // French: 2. d4 d5
    m.insert(
        "rnbqkbnr/ppp2ppp/4p3/3p4/3PP3/8/PPP2PPP/RNBQKBNR w",
        &["b1c3", "b1d2", "e4e5", "e4xd5"][..],
    );

    // Caro-Kann: 1. e4 c6
    m.insert(
        "rnbqkbnr/pp1ppppp/2p5/8/4P3/8/PPPP1PPP/RNBQKBNR w",
        &["d2d4", "b1c3", "g1f3"][..],
    );
    // Caro-Kann: 2. d4 d5
    m.insert(
        "rnbqkbnr/pp2pppp/2p5/3p4/3PP3/8/PPP2PPP/RNBQKBNR w",
        &["b1c3", "e4e5", "e4xd5", "b1d2"][..],
    );

    // --- 1. d4 openings ---
    m.insert(
        "rnbqkbnr/pppppppp/8/8/3P4/8/PPP1PPPP/RNBQKBNR b",
        &["d7d5", "g8f6", "e7e6", "f7f5", "c7c5"][..],
    );
    // 1. d4 d5
    m.insert(
        "rnbqkbnr/ppp1pppp/8/3p4/3P4/8/PPP1PPPP/RNBQKBNR w",
        &["c2c4", "g1f3", "c1f4", "e2e3"][..],
    );
    // Queen's Gambit: 1. d4 d5 2. c4
    m.insert(
        "rnbqkbnr/ppp1pppp/8/3p4/2PP4/8/PP2PPPP/RNBQKBNR b",
        &["e7e6", "c7c6", "d5xc4", "g8f6"][..],
    );
    // QGD: 2... e6 3. Nc3
    m.insert(
        "rnbqkbnr/ppp2ppp/4p3/3p4/2PP4/2N5/PP2PPPP/R1BQKBNR b",
        &["g8f6", "c7c6", "f8e7"][..],
    );
    // Slav Defense: 2... c6 3. Nf3
    m.insert(
        "rnbqkbnr/pp2pppp/2p5/3p4/2PP4/5N2/PP2PPPP/RNBQKB1R b",
        &["g8f6", "e7e6"][..],
    );

    // Indian Defenses: 1. d4 Nf6
    m.insert(
        "rnbqkb1r/pppppppp/5n2/8/3P4/8/PPP1PPPP/RNBQKBNR w",
        &["c2c4", "g1f3", "c1g5"][..],
    );
    // 1. d4 Nf6 2. c4
    m.insert(
        "rnbqkb1r/pppppppp/5n2/8/2PP4/8/PP2PPPP/RNBQKBNR b",
        &["e7e6", "g7g6", "c7c5", "e7e5"][..],
    );
    // King's Indian / Grunfeld: 2... g6 3. Nc3
    m.insert(
        "rnbqkb1r/pppppp1p/5np1/8/2PP4/2N5/PP2PPPP/R1BQKBNR b",
        &["d7d5", "f8g7"][..],
    );
    // Nimzo/Queen's Indian: 2... e6 3. Nc3
    m.insert(
        "rnbqkb1r/pppp1ppp/4pn2/8/2PP4/2N5/PP2PPPP/R1BQKBNR b",
        &["f8b4", "d7d5", "c7c5"][..],
    );

    // English Opening: 1. c4
    m.insert(
        "rnbqkbnr/pppppppp/8/8/2P5/8/PP1PPPPP/RNBQKBNR b",
        &["e7e5", "c7c5", "g8f6", "e7e6"][..],
    );
    // Reti Opening: 1. Nf3
    m.insert(
        "rnbqkbnr/pppppppp/8/8/8/5N2/PPPPPPPP/RNBQKB1R b",
        &["d7d5", "g8f6", "c7c5"][..],
    );

    m
}

/// Normalizes a FEN to its core placement and active turn fields.
pub fn normalize_fen_for_book(fen: &str) -> String {
    let parts: Vec<&str> = fen.split_whitespace().collect();
    if parts.len() >= 2 {
        format!("{} {}", parts[0], parts[1])
    } else {
        fen.trim().to_string()
    }
}

/// Looks up grandmaster theoretical opening moves for the given position.
pub fn get_book_moves(fen: &str) -> Option<Vec<String>> {
    let key = normalize_fen_for_book(fen);
    let book = BOOK.get_or_init(init_book);
    book.get(key.as_str())
        .map(|moves| moves.iter().map(|s| s.to_string()).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_starting_position_book() {
        let fen = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";
        let moves = get_book_moves(fen).expect("Start position must have book moves");
        assert!(moves.contains(&"e2e4".to_string()));
        assert!(moves.contains(&"d2d4".to_string()));
    }

    #[test]
    fn test_sicilian_response_book() {
        let fen = "rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq e3 0 1";
        let moves = get_book_moves(fen).expect("1.e4 must have book responses");
        assert!(moves.contains(&"c7c5".to_string()));
        assert!(moves.contains(&"e7e5".to_string()));
    }

    #[test]
    fn test_out_of_book_position() {
        let fen = "8/8/8/4k3/8/8/4K3/8 w - - 0 50";
        assert_eq!(get_book_moves(fen), None);
    }
}
