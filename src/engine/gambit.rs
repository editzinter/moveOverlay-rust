//! Prefer material offers only when Stockfish considers them close to its best move.
use shakmaty::{fen::Fen, uci::UciMove, CastlingMode, Chess, Position, Role};
use std::collections::BTreeMap;

fn value(role: Role) -> i32 {
    match role {
        Role::Pawn => 100,
        Role::Knight => 320,
        Role::Bishop => 330,
        Role::Rook => 500,
        Role::Queen => 900,
        Role::King => 0,
    }
}

/// Recognize an immediate offer of the moved piece, after accounting for the
/// initial capture and our best available recapture. Equal trades earn no bonus.
fn offer(pos: &Chess, m: &shakmaty::Move) -> i32 {
    let gain = m.capture().map(value).unwrap_or(0);
    let mut next = pos.clone();
    next.play_unchecked(m);
    next.legal_moves().iter().filter(|r| r.is_capture() && r.to() == m.to())
        .map(|reply| {
            let lost = reply.capture().map(value).unwrap_or(0);
            let mut accepted = next.clone();
            accepted.play_unchecked(reply);
            let recovered = accepted.legal_moves().iter()
                .filter(|r| r.is_capture() && r.to() == reply.to())
                .filter_map(|r| r.capture().map(value)).max().unwrap_or(0);
            (lost - gain - recovered).max(0)
        }).max().unwrap_or(0)
}

pub fn rank(fen: &str, moves: &[String], evaluations: &BTreeMap<String, i32>) -> Vec<String> {
    let Some(pos) = fen.parse::<Fen>().ok()
        .and_then(|f| f.into_position::<Chess>(CastlingMode::Standard).ok()) else {
        return Vec::new();
    };
    // A sacrifice is a stylistic tie-breaker, not compensation for a bad evaluation.
    // Scores are centipawns from the side to move. Missing scores cannot establish
    // that an offer is sound, so those candidates retain their engine order.
    const MAX_SACRIFICE_GAP_CP: i32 = 50;
    const SACRIFICE_BONUS_CP: i32 = MAX_SACRIFICE_GAP_CP + 1;
    let best_score = moves.iter().filter_map(|m| evaluations.get(m)).copied().max();
    let mut ranked = Vec::new();
    for (index, text) in moves.iter().enumerate() {
        let Some(m) = text.parse::<UciMove>().ok().and_then(|u| u.to_move(&pos).ok()) else { continue; };
        let score = match (evaluations.get(text).copied(), best_score) {
            (Some(base), Some(best)) if base.abs() < 90_000 && best.abs() < 90_000
                && best.saturating_sub(base) <= MAX_SACRIFICE_GAP_CP && offer(&pos, &m) > 0 => {
                    base + SACRIFICE_BONUS_CP
                }
            (Some(base), _) => base,
            (None, _) => -200_000 - index as i32,
        };
        ranked.push((score, index, text.clone()));
    }
    ranked.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    ranked.into_iter().map(|(_, _, m)| m).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefers_only_close_bishop_offers() {
        let fen = "6k1/5ppp/8/8/8/3B4/8/6K1 w - - 0 1";
        let moves = vec!["d3e4".into(), "d3h7".into()];
        let mut scores = BTreeMap::from([("d3e4".into(), 0), ("d3h7".into(), -150)]);
        assert_eq!(rank(fen, &moves, &scores)[0], "d3e4");
        scores.insert("d3h7".into(), -35);
        assert_eq!(rank(fen, &moves, &scores)[0], "d3h7");
        scores.insert("d3h7".into(), -900);
        assert_eq!(rank(fen, &moves, &scores)[0], "d3e4");
        scores.insert("d3h7".into(), -100_000);
        assert_eq!(rank(fen, &moves, &scores)[0], "d3e4");
    }

    #[test]
    fn black_sacrifices_and_winning_mates() {
        let fen = "6k1/8/3b4/8/8/8/5PPP/6K1 b - - 0 1";
        let moves = vec!["d6e5".into(), "d6h2".into()];
        let mut scores = BTreeMap::from([("d6e5".into(), 0), ("d6h2".into(), -35)]);
        assert_eq!(rank(fen, &moves, &scores)[0], "d6h2");
        scores.insert("d6e5".into(), 99_995);
        assert_eq!(rank(fen, &moves, &scores)[0], "d6e5");
        assert_eq!(rank(fen, &moves, &BTreeMap::new()), moves);
    }

    #[test]
    fn equal_trade_is_not_a_sacrifice() {
        let pos: Chess = "6k1/8/2p5/3p4/4P3/8/8/6K1 w - - 0 1".parse::<Fen>().unwrap().into_position(CastlingMode::Standard).unwrap();
        let m = "e4d5".parse::<UciMove>().unwrap().to_move(&pos).unwrap();
        assert_eq!(offer(&pos, &m), 0);
    }
}
