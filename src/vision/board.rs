use crate::vision::inference::Detection;
use shakmaty::{
    fen::Fen, uci::UciMove, Bitboard, Board, CastlingMode, Chess, Color, Piece, Position, Role,
    Setup, Square,
};

/// Maps YOLO class IDs to shakmaty Pieces.
pub fn class_to_piece(id: usize) -> Option<Piece> {
    match id {
        1 => Some(Piece {
            color: Color::White,
            role: Role::King,
        }),
        2 => Some(Piece {
            color: Color::White,
            role: Role::Queen,
        }),
        3 => Some(Piece {
            color: Color::White,
            role: Role::Rook,
        }),
        4 => Some(Piece {
            color: Color::White,
            role: Role::Bishop,
        }),
        5 => Some(Piece {
            color: Color::White,
            role: Role::Knight,
        }),
        6 => Some(Piece {
            color: Color::White,
            role: Role::Pawn,
        }),
        7 => Some(Piece {
            color: Color::Black,
            role: Role::King,
        }),
        8 => Some(Piece {
            color: Color::Black,
            role: Role::Queen,
        }),
        9 => Some(Piece {
            color: Color::Black,
            role: Role::Rook,
        }),
        10 => Some(Piece {
            color: Color::Black,
            role: Role::Bishop,
        }),
        11 => Some(Piece {
            color: Color::Black,
            role: Role::Knight,
        }),
        12 => Some(Piece {
            color: Color::Black,
            role: Role::Pawn,
        }),
        _ => None,
    }
}

/// Converts raw YOLO detections into a shakmaty 8x8 Board.
/// Uses direct normalized coordinates [0.0, 1.0] across the user-selected chessboard region.
pub fn detections_to_board(detections: &[Detection], play_as_black: bool) -> Option<Board> {
    let mut square_pieces: [Option<(Piece, f32)>; 64] = [None; 64];

    for d in detections {
        if d.class_id == 0 {
            continue; // Skip board bounding box
        }
        if let Some(piece) = class_to_piece(d.class_id) {
            let rel_x = d.bbox[0].clamp(0.0, 0.999);
            // Pieces are taller than wide, piece contact base sits slightly below bounding box center (+10% height)
            let rel_y = (d.bbox[1] + d.bbox[3] * 0.1).clamp(0.0, 0.999);

            let col_idx = (rel_x * 8.0).floor() as u32;
            let row_idx = (rel_y * 8.0).floor() as u32;

            if col_idx < 8 && row_idx < 8 {
                let (file_num, rank_num) = if play_as_black {
                    (7 - col_idx, row_idx)
                } else {
                    (col_idx, 7 - row_idx)
                };

                let sq_idx = (rank_num * 8 + file_num) as usize;
                if sq_idx < 64 {
                    match &square_pieces[sq_idx] {
                        Some((_, existing_conf)) if *existing_conf >= d.confidence => {}
                        _ => {
                            square_pieces[sq_idx] = Some((piece, d.confidence));
                        }
                    }
                }
            }
        }
    }

    let mut board = Board::empty();
    let mut white_king_count = 0;
    let mut black_king_count = 0;

    for (sq_idx, piece_opt) in square_pieces.iter().enumerate() {
        if let Some((piece, _)) = piece_opt {
            let file_num = (sq_idx % 8) as u32;
            let rank_num = (sq_idx / 8) as u32;
            let square =
                Square::from_coords(shakmaty::File::new(file_num), shakmaty::Rank::new(rank_num));
            board.set_piece_at(square, *piece);

            if piece.role == Role::King {
                if piece.color == Color::White {
                    white_king_count += 1;
                } else {
                    black_king_count += 1;
                }
            }
        }
    }

    if white_king_count != 1 || black_king_count != 1 {
        return None;
    }

    Some(board)
}

/// Converts detections to FEN without inferring unavailable castling history.
#[allow(dead_code)]
pub fn detections_to_fen(detections: &[Detection], play_as_black: bool) -> Option<String> {
    let board = detections_to_board(detections, play_as_black)?;
    let turn = if play_as_black {
        Color::Black
    } else {
        Color::White
    };

    let mut setup = Setup::empty();
    setup.board = board;
    setup.turn = turn;
    setup.castling_rights = Bitboard::EMPTY;
    setup.ep_square = None;

    let fen = Fen::from_setup(setup);
    Some(fen.to_string())
}

/// Robust Game State Tracker that synchronizes detected board states with chess rules and turn state.
pub struct GameStateTracker {
    pub current_pos: Option<Chess>,
    pub candidate_board: Option<Board>,
    pub candidate_count: u32,
    pub consecutive_mismatches: u32,
}

impl GameStateTracker {
    pub fn new() -> Self {
        Self {
            current_pos: None,
            candidate_board: None,
            candidate_count: 0,
            consecutive_mismatches: 0,
        }
    }

    pub fn reset(&mut self) {
        self.current_pos = None;
        self.candidate_board = None;
        self.candidate_count = 0;
        self.consecutive_mismatches = 0;
    }

    /// Feeds a newly detected board frame. Returns `Some(fen_to_analyze)` for the user's selected perspective.
    pub fn update(&mut self, detected_board: Board, user_is_black: bool) -> Option<String> {
        let user_color = if user_is_black {
            Color::Black
        } else {
            Color::White
        };

        // 2-frame debouncing to eliminate animation artifacts and detection flicker
        if self.candidate_board.as_ref() == Some(&detected_board) {
            self.candidate_count += 1;
        } else {
            self.candidate_board = Some(detected_board.clone());
            self.candidate_count = 1;
            return None; // Wait for stable second frame
        }

        if self.candidate_count < 2 {
            return None;
        }

        let settled_board = detected_board;

        let mut setup = Setup::empty();
        setup.board = settled_board;
        setup.turn = user_color;
        setup.castling_rights = Bitboard::EMPTY;
        setup.ep_square = None;

        let fen = Fen::from_setup(setup);
        Some(fen.to_string())
    }
}

/// Validates that proposed moves are strictly legal for the given FEN and belong to the user's selected piece color.
pub fn validate_moves_for_side(
    fen_str: &str,
    moves: &[String],
    user_is_black: bool,
) -> Vec<String> {
    let fen: Fen = match fen_str.parse() {
        Ok(f) => f,
        Err(_) => return Vec::new(),
    };
    let pos: Chess = match fen.into_position(CastlingMode::Standard) {
        Ok(p) => p,
        Err(_) => return Vec::new(),
    };

    let expected_color = if user_is_black {
        Color::Black
    } else {
        Color::White
    };
    if pos.turn() != expected_color {
        return Vec::new();
    }

    let legal_moves = pos.legal_moves();
    let mut valid_moves = Vec::new();

    for m_str in moves {
        if let Ok(uci) = m_str.parse::<UciMove>() {
            if let Ok(m) = uci.to_move(&pos) {
                if legal_moves.contains(&m) {
                    if let Some(from_sq) = m.from() {
                        if let Some(piece) = pos.board().piece_at(from_sq) {
                            if piece.color == expected_color {
                                valid_moves.push(m_str.clone());
                            }
                        }
                    }
                }
            }
        }
    }
    valid_moves
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_missing_kings_validation() {
        let detections = vec![Detection {
            class_id: 1, // White King only
            confidence: 0.95,
            bbox: [0.5, 0.9, 0.1, 0.1],
        }];
        assert_eq!(detections_to_board(&detections, false), None);
    }

    #[test]
    fn test_white_and_black_perspective() {
        let detections = vec![
            Detection {
                class_id: 1, // White King at e1
                confidence: 0.95,
                bbox: [0.56, 0.9, 0.08, 0.1],
            },
            Detection {
                class_id: 7, // Black King at e8
                confidence: 0.95,
                bbox: [0.56, 0.05, 0.08, 0.1],
            },
        ];

        let fen_white = detections_to_fen(&detections, false);
        assert!(fen_white.is_some());
        let fen_white_str = fen_white.unwrap();
        assert!(fen_white_str.contains(" w - - 0 1"));

        let fen_black = detections_to_fen(&detections, true);
        assert!(fen_black.is_some());
        let fen_black_str = fen_black.unwrap();
        assert!(fen_black_str.contains(" b - - 0 1"));
    }

    #[test]
    fn test_no_inferred_castling_rights() {
        let detections = vec![
            Detection {
                class_id: 1,
                confidence: 0.99,
                bbox: [0.56, 0.93, 0.08, 0.1],
            }, // Ke1
            Detection {
                class_id: 3,
                confidence: 0.99,
                bbox: [0.07, 0.93, 0.08, 0.1],
            }, // Ra1
            Detection {
                class_id: 3,
                confidence: 0.99,
                bbox: [0.93, 0.93, 0.08, 0.1],
            }, // Rh1
            Detection {
                class_id: 7,
                confidence: 0.99,
                bbox: [0.56, 0.07, 0.08, 0.1],
            }, // Ke8
            Detection {
                class_id: 9,
                confidence: 0.99,
                bbox: [0.07, 0.07, 0.08, 0.1],
            }, // Ra8
            Detection {
                class_id: 9,
                confidence: 0.99,
                bbox: [0.93, 0.07, 0.08, 0.1],
            }, // Rh8
        ];

        let fen = detections_to_fen(&detections, false).unwrap();
        assert!(
            fen.contains(" w - - 0 1"),
            "Must not infer castling rights without game history: {}",
            fen
        );
    }

    #[test]
    fn test_validate_moves_strict() {
        let fen = "rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b - - 0 1";
        let moves = vec![
            "c7c5".to_string(),
            "e7e5".to_string(),
            "e2e4".to_string(),
            "a1a8".to_string(),
        ];
        let valid_black = validate_moves_for_side(fen, &moves, true);
        assert_eq!(valid_black, vec!["c7c5", "e7e5"]);

        let valid_white = validate_moves_for_side(fen, &moves, false);
        assert!(valid_white.is_empty());
    }

    #[test]
    fn test_validate_moves_rejects_illegal_king_and_jumps() {
        let fen = "8/8/8/8/8/8/4P3/4K2k w - - 0 1";
        // e2e4 is legal, e1e3 is illegal (jump), e1a8 is illegal, h1h2 is black move
        let moves = vec![
            "e2e4".to_string(),
            "e1e3".to_string(),
            "e1a8".to_string(),
            "h1h2".to_string(),
        ];
        let valid = validate_moves_for_side(fen, &moves, false);
        assert_eq!(valid, vec!["e2e4"]);
    }

    #[test]
    fn test_game_state_tracker_debouncing_and_transition() {
        let mut tracker = GameStateTracker::new();

        // Build standard starting board
        let start_fen: Fen = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w - - 0 1"
            .parse()
            .unwrap();
        let start_pos: Chess = start_pos_from_fen(&start_fen);
        let start_board = start_pos.board().clone();

        // 1st frame: should return None because of 2-frame debouncing
        assert_eq!(tracker.update(start_board.clone(), false), None);

        // 2nd frame: stable board -> returns initial FEN (White's turn)
        let fen_opt = tracker.update(start_board.clone(), false);
        assert!(fen_opt.is_some());
        let fen_str = fen_opt.unwrap();
        assert!(fen_str.contains(" w - - 0 1"));

        // Play 1. e4 (White moves)
        let e4_fen: Fen = "rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b - - 0 1"
            .parse()
            .unwrap();
        let e4_pos: Chess = start_pos_from_fen(&e4_fen);
        let e4_board = e4_pos.board().clone();

        // 1st frame of e4: waiting for debounce
        assert_eq!(tracker.update(e4_board.clone(), false), None);

        // 2nd frame of e4: stable board -> returns FEN for user perspective (White)
        let e4_fen_opt = tracker.update(e4_board.clone(), false);
        assert!(e4_fen_opt.is_some());
        assert!(e4_fen_opt.unwrap().contains(" w - - 0 1"));

        // If user is Black: returns FEN for Black perspective
        tracker.reset();
        assert_eq!(tracker.update(e4_board.clone(), true), None);
        let black_turn_fen = tracker.update(e4_board, true);
        assert!(black_turn_fen.is_some());
        assert!(black_turn_fen.unwrap().contains(" b - - 0 1"));
    }

    #[test]
    fn test_pawn_and_piece_grid_coordinates_white_and_black() {
        // e2 pawn in white perspective: col 4 (0.5625), rank 2 (screen row 6: 0.8125)
        // e7 pawn in white perspective: col 4 (0.5625), rank 7 (screen row 1: 0.1875)
        let detections_white = vec![
            Detection {
                class_id: 1,
                confidence: 0.99,
                bbox: [0.5625, 0.9375, 0.08, 0.1],
            }, // Ke1
            Detection {
                class_id: 7,
                confidence: 0.99,
                bbox: [0.5625, 0.0625, 0.08, 0.1],
            }, // Ke8
            Detection {
                class_id: 6,
                confidence: 0.95,
                bbox: [0.5625, 0.8125, 0.08, 0.1],
            }, // Pe2
            Detection {
                class_id: 12,
                confidence: 0.95,
                bbox: [0.5625, 0.1875, 0.08, 0.1],
            }, // pe7
        ];

        let board_white = detections_to_board(&detections_white, false).unwrap();
        assert_eq!(
            board_white.piece_at(Square::E1),
            Some(Piece {
                color: Color::White,
                role: Role::King
            })
        );
        assert_eq!(
            board_white.piece_at(Square::E8),
            Some(Piece {
                color: Color::Black,
                role: Role::King
            })
        );
        assert_eq!(
            board_white.piece_at(Square::E2),
            Some(Piece {
                color: Color::White,
                role: Role::Pawn
            })
        );
        assert_eq!(
            board_white.piece_at(Square::E7),
            Some(Piece {
                color: Color::Black,
                role: Role::Pawn
            })
        );

        // Now test same physical piece positions when viewed from Black's perspective (flipped board):
        // In flipped board: top-left is h1 (col 0, row 0).
        // e7 is col 3 (h=0,g=1,f=2,e=3), row 6 (rank 7: 1=0..7=6). x = 0.4375, y = 0.8125
        // e2 is col 3, row 1 (rank 2: 1=0..2=1). x = 0.4375, y = 0.1875
        let detections_black = vec![
            Detection {
                class_id: 1,
                confidence: 0.99,
                bbox: [0.4375, 0.0625, 0.08, 0.1],
            }, // Ke1
            Detection {
                class_id: 7,
                confidence: 0.99,
                bbox: [0.4375, 0.9375, 0.08, 0.1],
            }, // Ke8
            Detection {
                class_id: 6,
                confidence: 0.95,
                bbox: [0.4375, 0.1875, 0.08, 0.1],
            }, // Pe2
            Detection {
                class_id: 12,
                confidence: 0.95,
                bbox: [0.4375, 0.8125, 0.08, 0.1],
            }, // pe7
        ];

        let board_black = detections_to_board(&detections_black, true).unwrap();
        assert_eq!(
            board_black.piece_at(Square::E1),
            Some(Piece {
                color: Color::White,
                role: Role::King
            })
        );
        assert_eq!(
            board_black.piece_at(Square::E8),
            Some(Piece {
                color: Color::Black,
                role: Role::King
            })
        );
        assert_eq!(
            board_black.piece_at(Square::E2),
            Some(Piece {
                color: Color::White,
                role: Role::Pawn
            })
        );
        assert_eq!(
            board_black.piece_at(Square::E7),
            Some(Piece {
                color: Color::Black,
                role: Role::Pawn
            })
        );
    }

    #[test]
    fn test_invalid_fen_and_color_mismatch() {
        // Invalid FEN string
        let invalid_fen = "not_a_valid_fen_string";
        let moves = vec!["e2e4".to_string()];
        assert!(validate_moves_for_side(invalid_fen, &moves, false).is_empty());

        // Black's turn FEN checked with White perspective
        let black_turn_fen = "4k3/8/8/8/8/8/4P3/4K3 b - - 0 1";
        assert!(validate_moves_for_side(black_turn_fen, &moves, false).is_empty());
    }

    fn start_pos_from_fen(fen: &Fen) -> Chess {
        fen.clone().into_position(CastlingMode::Standard).unwrap()
    }
}
