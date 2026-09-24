//! Rank sound Stockfish candidates by how much play they leave in the position.
use shakmaty::{
    fen::Fen, uci::UciMove, CastlingMode, Chess, Color, File, Position, Rank, Role, Square,
};
use std::collections::BTreeMap;

// A roughly equal position is acceptable even when Stockfish can win quickly.
// In a worse position, do not keep giving away evaluation just to play quietly.
const PLAYABLE_FLOOR_CP: i32 = -50;
const DEFENCE_LOSS_CP: i32 = 80;
const MATE_SCORE: i32 = 90_000;

fn material_imbalance(pos: &Chess) -> i32 {
    let mut white: i32 = 0;
    let mut black: i32 = 0;
    for file in 0..8 {
        for rank in 0..8 {
            let square = Square::from_coords(File::new(file), Rank::new(rank));
            if let Some(piece) = pos.board().piece_at(square) {
                let value = match piece.role {
                    Role::Pawn => 100,
                    Role::Knight => 320,
                    Role::Bishop => 330,
                    Role::Rook => 500,
                    Role::Queen => 900,
                    Role::King => 0,
                };
                if piece.color == Color::White {
                    white += value;
                } else {
                    black += value;
                }
            }
        }
    }
    (white - black).abs()
}

fn parsed_position(fen: &str) -> Option<Chess> {
    fen.parse::<Fen>()
        .ok()
        .and_then(|f| f.into_position::<Chess>(CastlingMode::Standard).ok())
}

fn is_checkmate_after(pos: &Chess, m: &shakmaty::Move) -> bool {
    let mut next = pos.clone();
    next.play_unchecked(m);
    next.is_checkmate()
}

fn is_game_over_after(pos: &Chess, m: &shakmaty::Move) -> bool {
    let mut next = pos.clone();
    next.play_unchecked(m);
    next.is_game_over()
}

/// Apply the no-checkmate rule even when Stockfish scores are too shallow to rank.
pub fn without_checkmates(fen: &str, moves: Vec<String>) -> Vec<String> {
    let Some(pos) = parsed_position(fen) else {
        return Vec::new();
    };
    moves
        .into_iter()
        .filter(|text| {
            text.parse::<UciMove>()
                .ok()
                .and_then(|u| u.to_move(&pos).ok())
                .is_some_and(|m| !is_checkmate_after(&pos, &m))
        })
        .collect()
}

/// Keep an immediate draw only when no legal move can continue the position.
pub fn without_avoidable_endings(fen: &str, moves: Vec<String>) -> Vec<String> {
    let Some(pos) = parsed_position(fen) else {
        return Vec::new();
    };
    if pos.legal_moves().iter().all(|m| is_game_over_after(&pos, m)) {
        return moves;
    }
    moves
        .into_iter()
        .filter(|text| {
            text.parse::<UciMove>()
                .ok()
                .and_then(|u| u.to_move(&pos).ok())
                .is_some_and(|m| !is_game_over_after(&pos, &m))
        })
        .collect()
}

/// Search every legal move when the engine's shortlist contained only endings.
pub fn fallback_nonmating_moves(fen: &str) -> Vec<String> {
    let Some(pos) = parsed_position(fen) else {
        return Vec::new();
    };
    let initial_locks = locked_pawn_pairs(&pos);
    let initial_reserves = pawn_push_reserves(&pos);
    let mut ranked = Vec::new();
    for m in pos.legal_moves() {
        let mut next = pos.clone();
        next.play_unchecked(&m);
        if next.is_checkmate() {
            continue;
        }
        let ends_game = next.is_game_over();
        let mate_reply = next
            .legal_moves()
            .iter()
            .any(|reply| is_checkmate_after(&next, reply));
        let score = longevity_score(&pos, &m, initial_locks, initial_reserves)
            - material_imbalance(&next) / 3;
        ranked.push((
            ends_game,
            mate_reply,
            -score,
            UciMove::from_move(&m, CastlingMode::Standard).to_string(),
        ));
    }
    ranked.sort();
    if ranked.iter().any(|entry| !entry.0) {
        ranked.retain(|entry| !entry.0);
    }
    ranked.into_iter().map(|(_, _, _, text)| text).collect()
}

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

fn pawn_push_reserves(pos: &Chess) -> i32 {
    let mut count = 0;
    for file in 0..8 {
        for rank in 0..8 {
            let square = Square::from_coords(File::new(file), Rank::new(rank));
            let Some(pawn) = pos
                .board()
                .piece_at(square)
                .filter(|p| p.role == Role::Pawn)
            else {
                continue;
            };
            let next_rank = match pawn.color {
                Color::White if rank < 7 => rank + 1,
                Color::Black if rank > 0 => rank - 1,
                _ => continue,
            };
            let ahead = Square::from_coords(File::new(file), Rank::new(next_rank));
            if pos.board().piece_at(ahead).is_none() {
                count += 1;
            }
        }
    }
    count
}

fn eligible(score: i32, best: i32) -> bool {
    if best < -MATE_SCORE {
        // When mate is unavoidable, choose the line that delays it longest.
        score < -MATE_SCORE
    } else if best >= PLAYABLE_FLOOR_CP {
        // Giving up a large *winning* advantage can prolong the game, provided
        // the alternative is still playable. Never choose a losing mate.
        score >= PLAYABLE_FLOOR_CP
    } else {
        score > -MATE_SCORE && best.saturating_sub(score) <= DEFENCE_LOSS_CP
    }
}

fn longevity_score(
    pos: &Chess,
    m: &shakmaty::Move,
    initial_locks: i32,
    initial_reserves: i32,
) -> i32 {
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
    if initial_reserves > 0 && pawn_push_reserves(&next) == 0 {
        // Keep at least one possible pawn advance available to reset the
        // fifty-move counter later. The vision FEN has no reliable history.
        score -= 120;
    }
    if next.is_check() {
        score -= 35;
    }
    let replies = next.legal_moves();
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
    let Some(pos) = parsed_position(fen) else {
        return Vec::new();
    };
    let has_continuing_candidate = moves.iter().any(|text| {
        text.parse::<UciMove>()
            .ok()
            .and_then(|u| u.to_move(&pos).ok())
            .is_some_and(|m| !is_game_over_after(&pos, &m))
    });
    let Some(best) = moves
        .iter()
        .filter(|text| {
            text.parse::<UciMove>()
                .ok()
                .and_then(|u| u.to_move(&pos).ok())
                .is_some_and(|m| {
                    !is_checkmate_after(&pos, &m)
                        && (!has_continuing_candidate || !is_game_over_after(&pos, &m))
                })
        })
        .filter_map(|m| evaluations.get(m))
        .copied()
        .max()
    else {
        return without_avoidable_endings(fen, without_checkmates(fen, moves.to_vec()));
    };
    let initial_locks = locked_pawn_pairs(&pos);
    let initial_reserves = pawn_push_reserves(&pos);
    let mut ranked = Vec::new();
    for (index, text) in moves.iter().enumerate() {
        let Some(m) = text
            .parse::<UciMove>()
            .ok()
            .and_then(|u| u.to_move(&pos).ok())
        else {
            continue;
        };
        if is_checkmate_after(&pos, &m) {
            continue;
        }
        if has_continuing_candidate && is_game_over_after(&pos, &m) {
            continue;
        }
        let Some(eval) = evaluations
            .get(text)
            .copied()
            .filter(|&s| eligible(s, best))
        else {
            continue;
        };
        let mut next = pos.clone();
        next.play_unchecked(&m);
        let terminal = next.is_game_over();
        let offered = crate::engine::gambit::offer(&pos, &m);
        let longevity = longevity_score(&pos, &m, initial_locks, initial_reserves);
        // Favor a near-equal continuation among otherwise similar moves. A
        // material-preserving quiet move can still beat an equalizing trade.
        let evaluation_balance_cost = if eval > MATE_SCORE {
            0
        } else if eval >= 0 {
            eval.min(1_000) / 5
        } else {
            eval.saturating_abs() / 2
        };
        let material_balance_cost = material_imbalance(&next) / 3;
        ranked.push((
            terminal,
            offered > 0,
            eval,
            longevity - evaluation_balance_cost - material_balance_cost,
            index,
            text.clone(),
        ));
    }
    ranked.sort_by(|a, b| {
        a.0.cmp(&b.0) // Continuing the game beats ending it this move.
            .then_with(|| a.1.cmp(&b.1)) // Avoid hanging material.
            .then_with(|| {
                if best < -MATE_SCORE {
                    b.2.cmp(&a.2) // Delay a forced loss.
                } else if a.2 > MATE_SCORE && b.2 <= MATE_SCORE {
                    std::cmp::Ordering::Greater
                } else if b.2 > MATE_SCORE && a.2 <= MATE_SCORE {
                    std::cmp::Ordering::Less
                } else if a.2 > MATE_SCORE && b.2 > MATE_SCORE {
                    a.2.cmp(&b.2) // Delay a forced win.
                } else if best < PLAYABLE_FLOOR_CP {
                    b.2.cmp(&a.2) // Defend a worse position first.
                } else {
                    b.3.cmp(&a.3).then_with(|| a.2.abs().cmp(&b.2.abs()))
                }
            })
            .then_with(|| a.4.cmp(&b.4))
    });
    ranked
        .into_iter()
        .map(|(_, _, _, _, _, text)| text)
        .collect()
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
        let fen = "6k1/8/4p2p/8/4P2P/8/8/6K1 w - - 0 1";
        let (moves, scores) = candidates(&[("g1f2", 10), ("e4e5", 0)]);
        assert_eq!(rank(fen, &moves, &scores)[0], "e4e5");
    }

    #[test]
    fn black_can_lock_pawns_too() {
        let fen = "6k1/8/8/4p2p/8/4P2P/8/6K1 b - - 0 1";
        let (moves, scores) = candidates(&[("g8f7", 10), ("e5e4", 0)]);
        assert_eq!(rank(fen, &moves, &scores)[0], "e5e4");
    }

    #[test]
    fn keeps_the_last_pawn_advance_in_reserve() {
        let fen = "6k1/8/4p3/8/4P3/8/8/6K1 w - - 0 1";
        let (moves, scores) = candidates(&[("e4e5", 0), ("g1f2", 0)]);
        assert_eq!(rank(fen, &moves, &scores)[0], "g1f2");
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
    fn gives_up_a_large_winning_edge_for_playable_longer_play() {
        let fen = "7k/5Q2/5K2/8/8/8/8/8 w - - 0 1";
        let (moves, scores) = candidates(&[("f7g7", 99_999), ("f7e6", 0)]);
        assert_eq!(rank(fen, &moves, &scores), vec!["f7e6"]);
    }

    #[test]
    fn skips_immediate_stalemate_even_if_it_looks_balanced() {
        let fen = "7k/5Q2/5K2/8/8/8/8/8 w - - 0 1";
        let (moves, scores) = candidates(&[("f7g6", 0), ("f7e6", 30)]);
        let mut pos: Chess = fen
            .parse::<Fen>()
            .unwrap()
            .into_position(CastlingMode::Standard)
            .unwrap();
        let stalemate = "f7g6".parse::<UciMove>().unwrap().to_move(&pos).unwrap();
        pos.play_unchecked(&stalemate);
        assert!(pos.is_stalemate());
        assert_eq!(rank(fen, &moves, &scores)[0], "f7e6");
    }

    #[test]
    fn avoids_stalemate_even_when_continuing_loses_evaluation() {
        let fen = "7k/5Q2/5K2/8/8/8/8/8 w - - 0 1";
        let (moves, scores) = candidates(&[("f7g6", 0), ("f7e6", -200)]);
        assert_eq!(rank(fen, &moves, &scores), vec!["f7e6"]);
        assert_eq!(
            without_avoidable_endings(fen, vec!["f7g6".into(), "f7e6".into()]),
            vec!["f7e6"]
        );
        assert!(without_avoidable_endings(fen, vec!["f7g6".into()]).is_empty());
        assert!(fallback_nonmating_moves(fen).iter().all(|text| text != "f7g6"));
    }

    #[test]
    fn avoids_a_material_offer_when_a_quiet_move_is_available() {
        let fen = "6k1/5ppp/8/8/8/3B4/8/6K1 w - - 0 1";
        let (moves, scores) = candidates(&[("d3h7", 0), ("d3e4", 60)]);
        assert_eq!(rank(fen, &moves, &scores)[0], "d3e4");
    }

    #[test]
    fn excludes_losing_alternatives_from_overlay_suggestions() {
        let fen = "6k1/8/8/3p4/4P3/8/8/6K1 w - - 0 1";
        let (moves, scores) = candidates(&[("e4d5", 0), ("g1f2", -200)]);
        assert_eq!(rank(fen, &moves, &scores), vec!["e4d5"]);
    }

    #[test]
    fn never_suggests_checkmate_even_when_the_alternative_is_worse() {
        let fen = "7k/5Q2/5K2/8/8/8/8/8 w - - 0 1";
        let (moves, scores) = candidates(&[("f7g7", 99_999), ("f7e6", -200)]);
        assert_eq!(rank(fen, &moves, &scores), vec!["f7e6"]);
        assert!(without_checkmates(fen, vec!["f7g7".into()]).is_empty());
        let fallback = fallback_nonmating_moves(fen);
        assert!(!fallback.is_empty());
        assert!(!fallback.contains(&"f7g7".to_string()));
    }

    #[test]
    fn equalizing_material_can_outweigh_one_capture() {
        let fen = "q5kr/8/8/8/8/8/8/R5K1 w - - 0 1";
        let (moves, scores) = candidates(&[("g1f2", 0), ("a1a8", 0)]);
        assert_eq!(rank(fen, &moves, &scores)[0], "a1a8");
    }

    #[test]
    fn checking_without_mating_is_allowed() {
        let fen = "6k1/8/8/8/8/8/8/R5K1 w - - 0 1";
        let (moves, scores) = candidates(&[("a1a8", 0)]);
        assert_eq!(rank(fen, &moves, &scores), vec!["a1a8"]);
    }

    #[test]
    fn missing_scores_keep_engine_order() {
        let fen = "6k1/8/8/8/8/8/8/6K1 w - - 0 1";
        let moves = vec!["g1f1".into(), "g1h1".into()];
        assert_eq!(rank(fen, &moves, &BTreeMap::new()), moves);
    }
}
