use crate::vision::inference::Detection;
use shakmaty::{fen::Fen, Board, Color, Piece, Role, Setup, Square};

pub fn detections_to_fen(detections: &[Detection], play_as_black: bool) -> Option<String> {
    let mut board = Board::empty();

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

    // Find the board bounding box to normalize coordinates
    let board_box = detections.iter().find(|d| d.class_id == 0);
    let (bx, by, bw, bh) = if let Some(b) = board_box {
        (
            b.bbox[0] - b.bbox[2] / 2.0,
            b.bbox[1] - b.bbox[3] / 2.0,
            b.bbox[2],
            b.bbox[3],
        )
    } else {
        (0.0, 0.0, 640.0, 640.0)
    };

    let mut white_king_count = 0;
    let mut black_king_count = 0;

    for d in detections {
        if d.class_id == 0 {
            continue;
        }
        if let Some(piece) = class_to_piece(d.class_id) {
            if piece.role == Role::King {
                if piece.color == Color::White {
                    white_king_count += 1;
                } else {
                    black_king_count += 1;
                }
            }

            // Calculate square from bbox
            let rel_x = (d.bbox[0] - bx) / bw;
            let rel_y = (d.bbox[1] - by) / bh;

            let col_idx = (rel_x * 8.0).floor() as i32;
            let row_idx = (rel_y * 8.0).floor() as i32;

            if (0..8).contains(&col_idx) && (0..8).contains(&row_idx) {
                let (file_num, rank_num) = if play_as_black {
                    // Board is flipped (Black perspective):
                    // Leftmost on screen is file H (7), rightmost is file A (0)
                    // Top on screen is rank 1 (0), bottom is rank 8 (7)
                    (7 - col_idx as u32, row_idx as u32)
                } else {
                    // Standard (White perspective):
                    // Leftmost on screen is file A (0), rightmost is file H (7)
                    // Top on screen is rank 8 (7), bottom is rank 1 (0)
                    (col_idx as u32, 7 - row_idx as u32)
                };

                let square = Square::from_coords(
                    shakmaty::File::new(file_num),
                    shakmaty::Rank::new(rank_num),
                );
                board.set_piece_at(square, piece);
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
                class_id: 0,
                confidence: 0.9,
                bbox: [320.0, 320.0, 640.0, 640.0],
            },
            // Only white king, no black king
            Detection {
                class_id: 1, // White King
                confidence: 0.95,
                bbox: [360.0, 600.0, 80.0, 80.0],
            },
        ];
        assert_eq!(detections_to_fen(&detections, false), None);
    }

    #[test]
    fn test_white_and_black_perspective() {
        let detections = vec![
            Detection {
                class_id: 0,
                confidence: 0.99,
                bbox: [320.0, 320.0, 640.0, 640.0],
            },
            // White King at bottom (center X, near bottom Y)
            // Rel X = 0.5 (col 4 -> file E), Rel Y = 7.0/8.0 (row 7 -> rank 1 for White, rank 8 for Black)
            Detection {
                class_id: 1, // White King
                confidence: 0.95,
                bbox: [360.0, 600.0, 80.0, 80.0],
            },
            // Black King at top (center X, near top Y)
            // Rel X = 0.5 (col 4 -> file E for White, file D for Black), Rel Y = 0.5/8.0 (row 0 -> rank 8 for White, rank 1 for Black)
            Detection {
                class_id: 7, // Black King
                confidence: 0.95,
                bbox: [360.0, 40.0, 80.0, 80.0],
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

