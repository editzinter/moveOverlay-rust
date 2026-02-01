use crate::yolo::Detection;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Orientation {
    WhiteBottom,
    BlackBottom,
}

#[derive(Debug, Clone, Copy)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

pub fn move_to_rect(
    move_str: &str,
    board_rect: Rect,
    orientation: Orientation,
) -> Option<(f32, f32, f32, f32)> {
    if move_str.len() < 4 {
        return None;
    }

    let chars: Vec<char> = move_str.chars().collect();
    let start_file = chars[0];
    let start_rank = chars[1];
    let end_file = chars[2];
    let end_rank = chars[3];

    // 'a' -> 0, 'b' -> 1 ...
    let mut start_col = (start_file as u8).checked_sub(b'a')? as f32;
    let mut end_col = (end_file as u8).checked_sub(b'a')? as f32;

    // '1' -> 7, '8' -> 0 (Standard WhiteBottom mapping)
    let mut start_row = (8 - start_rank.to_digit(10)?) as f32;
    let mut end_row = (8 - end_rank.to_digit(10)?) as f32;

    if orientation == Orientation::BlackBottom {
        // Flip horizontal and vertical
        start_col = 7.0 - start_col;
        end_col = 7.0 - end_col;
        start_row = 7.0 - start_row;
        end_row = 7.0 - end_row;
    }

    if start_col < 0.0 || start_col > 7.0 || start_row < 0.0 || start_row > 7.0 {
        return None;
    }
    if end_col < 0.0 || end_col > 7.0 || end_row < 0.0 || end_row > 7.0 {
        return None;
    }

    let cell_w = board_rect.w / 8.0;
    let cell_h = board_rect.h / 8.0;

    let x1 = board_rect.x + (start_col * cell_w) + (cell_w / 2.0);
    let y1 = board_rect.y + (start_row * cell_h) + (cell_h / 2.0);
    let x2 = board_rect.x + (end_col * cell_w) + (cell_w / 2.0);
    let y2 = board_rect.y + (end_row * cell_h) + (cell_h / 2.0);

    Some((x1, y1, x2, y2))
}

pub fn detect_orientation(detections: &[Detection], _board_rect: Rect) -> Orientation {
    // Class 6: White Pawn
    // Class 12: Black Pawn
    let mut white_y_sum = 0.0;
    let mut white_count = 0;
    let mut black_y_sum = 0.0;
    let mut black_count = 0;

    for det in detections {
        if det.class_id == 6 {
            white_y_sum += det.y;
            white_count += 1;
        } else if det.class_id == 12 {
            black_y_sum += det.y;
            black_count += 1;
        }
    }

    if white_count == 0 || black_count == 0 {
        return Orientation::WhiteBottom;
    }

    let avg_white_y = white_y_sum / white_count as f32;
    let avg_black_y = black_y_sum / black_count as f32;

    // Higher Y value means lower on the screen
    if avg_white_y > avg_black_y {
        Orientation::WhiteBottom
    } else {
        Orientation::BlackBottom
    }
}

pub fn detections_to_fen(detections: &[Detection], orientation: Orientation) -> (String, Rect) {
    // 1. Find the board (Class 0)
    let board = detections
        .iter()
        .filter(|d| d.class_id == 0 && d.w > 0.2 && d.h > 0.2) // Filter small garbage detections
        .max_by(|a, b| {
            a.confidence
                .partial_cmp(&b.confidence)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

    let board = match board {
        Some(b) => b,
        None => {
            return (
                "8/8/8/8/8/8/8/8".to_string(),
                Rect {
                    x: 0.,
                    y: 0.,
                    w: 0.,
                    h: 0.,
                },
            )
        }
    };

    let (bx1, by1, bx2, by2) = board.bounds();
    let board_w = bx2 - bx1;
    let board_h = by2 - by1;
    let cell_w = board_w / 8.0;
    let cell_h = board_h / 8.0;

    let mut grid: [[Option<char>; 8]; 8] = [[None; 8]; 8];

    // Helper to store potential pieces before finalizing
    struct RawPiece {
        row: usize,
        col: usize,
        class_id: usize,
        #[allow(dead_code)]
        confidence: f32,
    }
    let mut raw_pieces = Vec::new();

    for det in detections {
        if det.class_id == 0 {
            continue;
        }

        let cx = det.x;
        let cy = det.y;

        if cx < bx1 || cx > bx2 || cy < by1 || cy > by2 {
            continue;
        }

        // Visual row/col (0,0 is Top-Left)
        let v_col = ((cx - bx1) / cell_w).floor() as usize;
        let v_row = ((cy - by1) / cell_h).floor() as usize;

        if v_col < 8 && v_row < 8 {
            let (row, col) = match orientation {
                Orientation::WhiteBottom => (v_row, v_col),
                Orientation::BlackBottom => (7 - v_row, 7 - v_col),
            };

            raw_pieces.push(RawPiece {
                row,
                col,
                class_id: det.class_id,
                confidence: det.confidence,
            });
        }
    }

    // --- HEURISTIC FIX: DUPLICATE KINGS ---
    // If we detect 2 White Kings and 0 Black Kings, force the one on the "Black side" to be Black.
    let white_kings: Vec<usize> = raw_pieces
        .iter()
        .enumerate()
        .filter(|(_, p)| p.class_id == 1)
        .map(|(i, _)| i)
        .collect();
    let black_kings: Vec<usize> = raw_pieces
        .iter()
        .enumerate()
        .filter(|(_, p)| p.class_id == 7)
        .map(|(i, _)| i)
        .collect();

    if white_kings.len() >= 2 && black_kings.is_empty() {
        // Find the king that is physically closest to the black side.
        // Rank 8 (Top) is row 0. Rank 1 (Bottom) is row 7.
        // We want the king with the smallest row index (closest to 0).
        let mut min_row = 999;
        let mut target_idx = 999;

        for &idx in &white_kings {
            if raw_pieces[idx].row < min_row {
                min_row = raw_pieces[idx].row;
                target_idx = idx;
            }
        }

        if target_idx != 999 {
            raw_pieces[target_idx].class_id = 7; // Convert to Black King
        }
    }

    // Fill Grid
    for p in raw_pieces {
        let piece_char = class_id_to_fen(p.class_id);
        grid[p.row][p.col] = Some(piece_char);
    }

    // Construct FEN Placement
    let mut fen_parts = Vec::new();
    for row in 0..8 {
        let mut empty_count = 0;
        let mut row_str = String::new();

        for col in 0..8 {
            match grid[row][col] {
                Some(p) => {
                    if empty_count > 0 {
                        row_str.push_str(&empty_count.to_string());
                        empty_count = 0;
                    }
                    row_str.push(p);
                }
                None => empty_count += 1,
            }
        }
        if empty_count > 0 {
            row_str.push_str(&empty_count.to_string());
        }
        fen_parts.push(row_str);
    }

    let placement = fen_parts.join("/");
    (
        placement,
        Rect {
            x: bx1,
            y: by1,
            w: board_w,
            h: board_h,
        },
    )
}

fn class_id_to_fen(id: usize) -> char {
    match id {
        1 => 'K',
        2 => 'Q',
        3 => 'R',
        4 => 'B',
        5 => 'N',
        6 => 'P', // White
        7 => 'k',
        8 => 'q',
        9 => 'r',
        10 => 'b',
        11 => 'n',
        12 => 'p', // Black
        _ => '?',
    }
}
