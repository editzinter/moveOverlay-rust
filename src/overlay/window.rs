use crate::config::BoardRegion;
use eframe::egui;

/// Converts a physical virtual-screen BoardRegion into egui logical viewport Rect.
pub fn board_region_to_egui_rect(
    region: &BoardRegion,
    window_origin: (i32, i32),
    ppp: f32,
) -> egui::Rect {
    let scale = if ppp > 0.0 { ppp } else { 1.0 };
    let min_x = (region.x - window_origin.0) as f32 / scale;
    let min_y = (region.y - window_origin.1) as f32 / scale;
    let width = region.width as f32 / scale;
    let height = region.height as f32 / scale;

    egui::Rect::from_min_size(egui::pos2(min_x, min_y), egui::vec2(width, height))
}

/// Converts an egui logical viewport Rect into a physical virtual-screen BoardRegion.
pub fn egui_rect_to_board_region(
    rect: egui::Rect,
    window_origin: (i32, i32),
    ppp: f32,
) -> BoardRegion {
    let scale = if ppp > 0.0 { ppp } else { 1.0 };
    let min_x = rect.min.x.min(rect.max.x);
    let min_y = rect.min.y.min(rect.max.y);
    let width = rect.width().abs();
    let height = rect.height().abs();

    let phys_x = window_origin.0 + (min_x * scale).round() as i32;
    let phys_y = window_origin.1 + (min_y * scale).round() as i32;
    let phys_w = (width * scale).round().max(1.0) as u32;
    let phys_h = (height * scale).round().max(1.0) as u32;

    BoardRegion {
        x: phys_x,
        y: phys_y,
        width: phys_w,
        height: phys_h,
    }
}

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

    let outline_color = egui::Color32::from_black_alpha((color.a() as f32 * 0.85) as u8);
    let outline_width = thickness + 3.0;

    // 1. Draw origin piece indicator circle (base marker)
    let dot_radius = thickness * 0.8;
    painter.circle_filled(start, dot_radius + 1.5, outline_color);
    painter.circle_filled(start, dot_radius, color);

    // 2. Draw outline (Shaft & Head) for maximum contrast
    painter.line_segment(
        [start, shaft_end],
        egui::Stroke::new(outline_width, outline_color),
    );

    let p1 = end - unit_dir * head_size + norm * head_width;
    let p2 = end - unit_dir * head_size - norm * head_width;
    let p_indent = end - unit_dir * (head_size * 0.75);

    let head_poly = vec![end, p1, p_indent, p2];
    painter.add(egui::Shape::convex_polygon(
        head_poly.clone(),
        outline_color,
        egui::Stroke::new(3.0, outline_color),
    ));

    // 3. Draw foreground arrow (Shaft & Head)
    painter.line_segment([start, shaft_end], egui::Stroke::new(thickness, color));
    painter.add(egui::Shape::convex_polygon(
        head_poly,
        color,
        egui::Stroke::NONE,
    ));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_square_positions_white() {
        let rect = egui::Rect::from_min_size(egui::pos2(100.0, 200.0), egui::vec2(800.0, 800.0));
        // a1 is bottom-left (x: 100 + 50 = 150, y: 200 + 750 = 950)
        let a1 = square_to_screen_pos("a1", rect, false).unwrap();
        assert_eq!(a1, egui::pos2(150.0, 950.0));

        // h8 is top-right (x: 100 + 750 = 850, y: 200 + 50 = 250)
        let h8 = square_to_screen_pos("h8", rect, false).unwrap();
        assert_eq!(h8, egui::pos2(850.0, 250.0));

        // e4 is col 4 ('e'), row 3 ('4') -> draw_col=4, draw_row=4 -> x: 100 + 450 = 550, y: 200 + 450 = 650
        let e4 = square_to_screen_pos("e4", rect, false).unwrap();
        assert_eq!(e4, egui::pos2(550.0, 650.0));
    }

    #[test]
    fn test_square_positions_black() {
        let rect = egui::Rect::from_min_size(egui::pos2(100.0, 200.0), egui::vec2(800.0, 800.0));
        // When playing black (flipped), a1 is top-right (x: 100 + 750 = 850, y: 200 + 50 = 250)
        let a1 = square_to_screen_pos("a1", rect, true).unwrap();
        assert_eq!(a1, egui::pos2(850.0, 250.0));

        // When playing black (flipped), h8 is bottom-left (x: 100 + 50 = 150, y: 200 + 750 = 950)
        let h8 = square_to_screen_pos("h8", rect, true).unwrap();
        assert_eq!(h8, egui::pos2(150.0, 950.0));

        // e5 in black: col 4 ('e'), row 4 ('5') -> draw_col=7-4=3, draw_row=4 -> x: 100 + 350 = 450, y: 200 + 450 = 650
        let e5 = square_to_screen_pos("e5", rect, true).unwrap();
        assert_eq!(e5, egui::pos2(450.0, 650.0));
    }

    #[test]
    fn test_coordinate_mapping_dpi_and_nonzero_origin() {
        let origin = (1920, 0); // Monitor 2
        let ppp = 1.5; // 150% scaling

        let original_region = BoardRegion {
            x: 2220, // 300px to the right of monitor 2's left
            y: 300,
            width: 900,
            height: 900,
        };

        let rect = board_region_to_egui_rect(&original_region, origin, ppp);
        // Logical X should be (2220 - 1920) / 1.5 = 300 / 1.5 = 200.0
        assert_eq!(rect.min.x, 200.0);
        // Logical Y should be 300 / 1.5 = 200.0
        assert_eq!(rect.min.y, 200.0);
        // Logical Width should be 900 / 1.5 = 600.0
        assert_eq!(rect.width(), 600.0);
        assert_eq!(rect.height(), 600.0);

        // Roundtrip conversion back to BoardRegion
        let recovered = egui_rect_to_board_region(rect, origin, ppp);
        assert_eq!(recovered, original_region);
    }

    #[test]
    fn test_coordinate_mapping_negative_screen_origin() {
        let origin = (-1920, -1080); // Secondary monitor on top-left
        let ppp = 1.25;

        let original_region = BoardRegion {
            x: -1600,
            y: -800,
            width: 750,
            height: 750,
        };

        let rect = board_region_to_egui_rect(&original_region, origin, ppp);
        let recovered = egui_rect_to_board_region(rect, origin, ppp);
        assert_eq!(recovered, original_region);
    }
}
