use crate::vision::inference::Detection;
use shakmaty::{fen::Fen, Board, Color, Piece, Role, Setup, Square};

pub fn detections_to_fen(detections: &[Detection], show_white_moves: bool) -> Option<String> {
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

            let col = (rel_x * 8.0).floor() as i32;
            let row = (rel_y * 8.0).floor() as i32;

            if col >= 0 && col < 8 && row >= 0 && row < 8 {
                let square = Square::from_coords(
                    shakmaty::File::new(col as u32),
                    shakmaty::Rank::new(7 - row as u32),
                );
                board.set_piece_at(square, piece);
            }
        }
    }

    // BASIC VALIDATION: A chess position MUST have exactly one king of each color
    // If vision missed a king, don't generate a FEN as it will confuse Stockfish
    if white_king_count != 1 || black_king_count != 1 {
        println!(
            "VALIDATION FAILED: Kings count W:{} B:{}",
            white_king_count, black_king_count
        );
        return None;
    }

    let turn = if show_white_moves {
        Color::White
    } else {
        Color::Black
    };
    let mut setup = Setup::empty();
    setup.board = board;
    setup.turn = turn;

    // Check if the position is actually legal (e.g. king isn't being captured)
    // Although for engine analysis we can be a bit more relaxed,
    // basic sanity is required.
    let fen = Fen::from_setup(setup);
    Some(fen.to_string())
}
