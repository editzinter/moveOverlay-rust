//! Rank sound Stockfish candidates by how much play they leave in the position.
use shakmaty::{
    fen::Fen, uci::UciMove, CastlingMode, Chess, Color, File, Position, Rank, Role, Square,
};
use std::collections::BTreeMap;

const MAX_EVAL_LOSS_CP: i32 = 80;

fn locked_pawn_pairs(pos: &Chess) -> i32 {
    let mut count = 0;
    for file in 0..8 {
        for rank in 0..7 {
            let white_sq = Square::from_coords(File::new(file), Rank::new(rank));
            let black_sq = Square::from_coords(File::new(file), Rank::new(rank + 1));
            if pos
                .board()
                .piece_at(white_sq)
                .is_some_and(|p| p.color == Color::White && p.role == Role::Pawn)
                && pos
                    .board()
                    .piece_at(black_sq)
                    .is_some_and(|p| p.color == Color::Black && p.role == Role::Pawn)
            {
                count += 1;
            }
        }
    }
    count
}

fn eligible(score: i32, best: i32) -> bool {
    if best > 90_000 {
        // A clearly winning, non-forcing line can be preferable to a quick mate.
        score > 90_000 || (100..90_000).contains(&score)
    } else if best < -90_000 {
        // When mate is unavoidable, choose the line that delays it longest.
        score < -90_000
    } else {
        score > -90_000 && best.saturating_sub(score) <= MAX_EVAL_LOSS_CP
    }
}

fn longevity_score(pos: &Chess, m: &shakmaty::Move, initial_locks: i32) -> i32 {
    let mut score = 0;
    if let Some(captured) = m.capture() {
        // Trading pieces and especially pawns reduces the number of moves left.
        score -= match captured {
            Role::Pawn => 110,
            Role::Queen => 170,
            Role::Rook => 130,
            _ => 100,
        };
    } else {
        score += 35;
    }
    if m.promotion().is_some() {
        score -= 120;
    }
    if m.from()
        .and_then(|sq| pos.board().piece_at(sq))
        .is_some_and(|p| p.role == Role::Pawn)
    {
        score -= 8;
    }

    let mut next = pos.clone();
    next.play_unchecked(m);
    score += (locked_pawn_pairs(&next) - initial_locks) * 80;
    if next.is_check() {
        score -= 35;
    }
    let replies = next.legal_moves();
    if replies.is_empty() {
        // An immediate checkmate or stalemate ends the game now.
        score -= 500;
    }
    // Avoid offering immediate pawn exchanges or simplifying recaptures.
    let pawn_captures = replies
        .iter()
        .filter(|reply| reply.capture() == Some(Role::Pawn))
        .count();
    let other_captures = replies
        .iter()
        .filter(|reply| reply.is_capture() && reply.capture() != Some(Role::Pawn))
        .count();
    score -= (pawn_captures.min(4) as i32) * 35;
    score -= (other_captures.min(4) as i32) * 10;
    score
}

pub fn rank(fen: &str, moves: &[String], evaluations: &BTreeMap<String, i32>) -> Vec<String> {
    let Some(pos) = fen
        .parse::<Fen>()
        .ok()
        .and_then(|f| f.into_position::<Chess>(CastlingMode::Standard).ok())
    else {
        return moves.to_vec();
    };
    let Some(best) = moves
        .iter()
        .filter_map(|m| evaluations.get(m))
        .copied()
        .max()
    else {
        return moves.to_vec();
    };
    let initial_locks = locked_pawn_pairs(&pos);
    let mut ranked = Vec::new();
    for (index, text) in moves.iter().enumerate() {
        let Some(m) = text
            .parse::<UciMove>()
            .ok()
            .and_then(|u| u.to_move(&pos).ok())
        else {
            continue;
        };
        let eval = evaluations.get(text).copied();
        let safe = eval.is_some_and(|score| eligible(score, best));
        let longevity = if safe {
            longevity_score(&pos, &m, initial_locks)
        } else {
            0
        };
        ranked.push((
            safe,
            longevity,
            eval.unwrap_or(-200_000),
            index,
            text.clone(),
        ));
    }
    ranked.sort_by(|a, b| {
        b.0.cmp(&a.0)
            .then_with(|| {
                if a.0 && b.0 {
                    // Among proven winning mates, the larger mate distance lasts longer.
                    // Among losing mates, Stockfish's larger score also delays mate.
                    if a.2 > 90_000 && b.2 <= 90_000 {
                        std::cmp::Ordering::Greater
                    } else if b.2 > 90_000 && a.2 <= 90_000 {
                        std::cmp::Ordering::Less
                    } else if a.2 > 90_000 && b.2 > 90_000 {
                        a.2.cmp(&b.2)
                    } else if a.2 < -90_000 && b.2 < -90_000 {
                        b.2.cmp(&a.2)
                    } else {
                        b.1.cmp(&a.1).then_with(|| b.2.cmp(&a.2))
                    }
                } else {
                    b.2.cmp(&a.2)
                }
            })
            .then_with(|| a.3.cmp(&b.3))
    });
    ranked.into_iter().map(|(_, _, _, _, text)| text).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidates(items: &[(&str, i32)]) -> (Vec<String>, BTreeMap<String, i32>) {
        (
            items.iter().map(|(m, _)| (*m).into()).collect(),
            items.iter().map(|(m, s)| ((*m).into(), *s)).collect(),
        )
    }

    #[test]
    fn keeps_pawns_when_evaluations_are_close() {
        let fen = "6k1/8/8/3p4/4P3/8/8/6K1 w - - 0 1";
        let (moves, scores) = candidates(&[("e4d5", 15), ("g1f2", 0)]);
        assert_eq!(rank(fen, &moves, &scores)[0], "g1f2");
    }

    #[test]
    fn favors_locking_pawns_over_idle_move() {
        let fen = "6k1/8/4p3/8/4P3/8/8/6K1 w - - 0 1";
        let (moves, scores) = candidates(&[("g1f2", 10), ("e4e5", 0)]);
        assert_eq!(rank(fen, &moves, &scores)[0], "e4e5");
    }

    #[test]
    fn black_can_lock_pawns_too() {
        let fen = "6k1/8/8/4p3/8/4P3/8/6K1 b - - 0 1";
        let (moves, scores) = candidates(&[("g8f7", 10), ("e5e4", 0)]);
        assert_eq!(rank(fen, &moves, &scores)[0], "e5e4");
    }

    #[test]
    fn refuses_a_large_evaluation_loss() {
        let fen = "6k1/8/8/3p4/4P3/8/8/6K1 w - - 0 1";
        let (moves, scores) = candidates(&[("e4d5", 0), ("g1f2", -200)]);
        assert_eq!(rank(fen, &moves, &scores)[0], "e4d5");
    }

    #[test]
    fn delays_forced_mate() {
        let fen = "6k1/8/8/8/8/8/8/6K1 w - - 0 1";
        let (moves, scores) = candidates(&[("g1f1", -99_998), ("g1h1", -99_992)]);
        assert_eq!(rank(fen, &moves, &scores)[0], "g1h1");
        let (moves, scores) = candidates(&[("g1f1", 99_999), ("g1h1", 99_995)]);
        assert_eq!(rank(fen, &moves, &scores)[0], "g1h1");
    }

    #[test]
    fn prefers_sound_quiet_play_to_quick_win() {
        let fen = "7k/5Q2/5K2/8/8/8/8/8 w - - 0 1";
        let (moves, scores) = candidates(&[("f7g7", 99_999), ("f7e6", 250)]);
        assert_eq!(rank(fen, &moves, &scores)[0], "f7e6");
    }

    #[test]
    fn missing_scores_keep_engine_order() {
        let fen = "6k1/8/8/8/8/8/8/6K1 w - - 0 1";
        let moves = vec!["g1f1".into(), "g1h1".into()];
        assert_eq!(rank(fen, &moves, &BTreeMap::new()), moves);
    }
}
