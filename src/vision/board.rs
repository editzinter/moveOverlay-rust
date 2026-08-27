use crate::vision::inference::Detection;
use shakmaty::{fen::Fen, Board, Color, Piece, Role, Setup, Square};

pub fn detections_to_fen(detections: &[Detection], play_as_black: bool) -> Option<String> {
    // Map class_id to Piece
    let class_to_piece = |id: usize| -> Option<Piece> {
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
    };

    // Track highest-confidence piece per square (64 squares) to prevent collision overwrite
    let mut square_pieces: [Option<(Piece, f32)>; 64] = [None; 64];

    for d in detections {
        if d.class_id == 0 {
            continue; // Skip board box
        }
        if let Some(piece) = class_to_piece(d.class_id) {
            // Bbox coordinates are normalized [0.0, 1.0] relative to the captured chessboard
            let rel_x = d.bbox[0].clamp(0.0, 0.999);
            // Pieces are taller than wide, base sits slightly below vertical center
            let rel_y = (d.bbox[1] + d.bbox[3] * 0.1).clamp(0.0, 0.999);

            let col_idx = (rel_x * 8.0).floor() as u32;
            let row_idx = (rel_y * 8.0).floor() as u32;

            if col_idx < 8 && row_idx < 8 {
                let (file_num, rank_num) = if play_as_black {
                    // Board is flipped (Black perspective):
                    // Leftmost on screen is file H (7), rightmost is file A (0)
                    // Top on screen is rank 1 (0), bottom is rank 8 (7)
                    (7 - col_idx, row_idx)
                } else {
                    // Standard (White perspective):
                    // Leftmost on screen is file A (0), rightmost is file H (7)
                    // Top on screen is rank 8 (7), bottom is rank 1 (0)
                    (col_idx, 7 - row_idx)
                };

                let sq_idx = (rank_num * 8 + file_num) as usize;
                if sq_idx < 64 {
                    match &square_pieces[sq_idx] {
                        Some((_, existing_conf)) if *existing_conf >= d.confidence => {
                            // Retain higher confidence piece
                        }
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
            let square = Square::from_coords(
                shakmaty::File::new(file_num),
                shakmaty::Rank::new(rank_num),
            );
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

    // Validation: A valid position requires exactly one white and one black king
    if white_king_count != 1 || black_king_count != 1 {
        return None;
    }

    let turn = if play_as_black {
        Color::Black
    } else {
        Color::White
    };

    let mut setup = Setup::empty();
    setup.board = board;
    setup.turn = turn;

    let fen = Fen::from_setup(setup);
    Some(fen.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_missing_kings_validation() {
        let detections = vec![
            Detection {
                class_id: 1, // White King only
                confidence: 0.95,
                bbox: [0.5, 0.9, 0.1, 0.1],
            },
        ];
        assert_eq!(detections_to_fen(&detections, false), None);
    }

    #[test]
    fn test_white_and_black_perspective() {
        let detections = vec![
            // White King at e1
            Detection {
                class_id: 1, // White King
                confidence: 0.95,
                bbox: [0.56, 0.9, 0.08, 0.1],
            },
            // Black King at e8
            Detection {
                class_id: 7, // Black King
                confidence: 0.95,
                bbox: [0.56, 0.05, 0.08, 0.1],
            },
        ];

        let fen_white = detections_to_fen(&detections, false);
        assert!(fen_white.is_some());
        let fen_white_str = fen_white.unwrap();
        assert!(fen_white_str.ends_with(" w - - 0 1"));

        let fen_black = detections_to_fen(&detections, true);
        assert!(fen_black.is_some());
        let fen_black_str = fen_black.unwrap();
        assert!(fen_black_str.ends_with(" b - - 0 1"));
    }
}
