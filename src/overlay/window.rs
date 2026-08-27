use eframe::egui;

pub fn square_to_screen_pos(sq: &str, rect: egui::Rect, play_as_black: bool) -> Option<egui::Pos2> {
    if sq.len() < 2 {
        return None;
    }
    let mut chars = sq.chars();
    let col_char = chars.next()?;
    let row_char = chars.next()?;

    let col = col_char.to_ascii_lowercase() as i32 - 'a' as i32;
    let row = row_char.to_digit(10)? as i32 - 1;

    if !(0..8).contains(&col) || !(0..8).contains(&row) {
        return None;
    }

    let cell_w = rect.width() / 8.0;
    let cell_h = rect.height() / 8.0;

    let (draw_col, draw_row) = if play_as_black {
        (7 - col, row)
    } else {
        (col, 7 - row)
    };

    Some(egui::pos2(
        rect.min.x + (draw_col as f32 + 0.5) * cell_w,
        rect.min.y + (draw_row as f32 + 0.5) * cell_h,
    ))
}

pub fn draw_arrow(
    painter: &egui::Painter,
    rect: egui::Rect,
    m: &str,
    color: egui::Color32,
    play_as_black: bool,
    thickness: f32,
) {
    if m.len() < 4 {
        return;
    }
    let from_sq = &m[0..2];
    let to_sq = &m[2..4];

    let start = match square_to_screen_pos(from_sq, rect, play_as_black) {
        Some(pos) => pos,
        None => return,
    };
    let end = match square_to_screen_pos(to_sq, rect, play_as_black) {
        Some(pos) => pos,
        None => return,
    };

    let dir = end - start;
    let length = dir.length();
    if length < 1.0 {
        return;
    }
    let unit_dir = dir / length;
    let norm = egui::vec2(-unit_dir.y, unit_dir.x);

    let head_size = (thickness * 2.8).clamp(16.0, 36.0);
    let head_width = head_size * 0.75;
    let shaft_end = if length > head_size * 0.7 {
        end - unit_dir * (head_size * 0.5)
    } else {
        end
    };

    let outline_color = egui::Color32::from_black_alpha((color.a() as f32 * 0.8) as u8);
    let outline_width = thickness + 3.0;

    // 1. Draw outline (Shaft & Head) for maximum contrast
    painter.line_segment([start, shaft_end], egui::Stroke::new(outline_width, outline_color));

    let p1 = end - unit_dir * head_size + norm * head_width;
    let p2 = end - unit_dir * head_size - norm * head_width;
    let p_indent = end - unit_dir * (head_size * 0.75);

    let outline_head = vec![end, p1, p_indent, p2];
    painter.add(egui::Shape::convex_polygon(
        outline_head.clone(),
        outline_color,
        egui::Stroke::new(1.5, outline_color),
    ));

    // 2. Draw foreground arrow (Shaft & Head)
    painter.line_segment([start, shaft_end], egui::Stroke::new(thickness, color));
    painter.add(egui::Shape::convex_polygon(
        outline_head,
        color,
        egui::Stroke::NONE,
    ));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_square_positions_white() {
        let rect = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(800.0, 800.0));
        // a1 is bottom-left (x: 50, y: 750)
        let a1 = square_to_screen_pos("a1", rect, false).unwrap();
        assert_eq!(a1, egui::pos2(50.0, 750.0));

        // h8 is top-right (x: 750, y: 50)
        let h8 = square_to_screen_pos("h8", rect, false).unwrap();
        assert_eq!(h8, egui::pos2(750.0, 50.0));
    }

    #[test]
    fn test_square_positions_black() {
        let rect = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(800.0, 800.0));
        // When playing black (flipped), a1 is top-right (x: 750, y: 50)
        let a1 = square_to_screen_pos("a1", rect, true).unwrap();
        assert_eq!(a1, egui::pos2(750.0, 50.0));

        // When playing black (flipped), h8 is bottom-left (x: 50, y: 750)
        let h8 = square_to_screen_pos("h8", rect, true).unwrap();
        assert_eq!(h8, egui::pos2(50.0, 750.0));
    }
}
