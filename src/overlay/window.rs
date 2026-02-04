use eframe::egui;

pub fn draw_arrow(painter: &egui::Painter, rect: egui::Rect, m: &str, color: egui::Color32) {
    if m.len() < 4 {
        return;
    }
    let from_sq = &m[0..2];
    let to_sq = &m[2..4];

    let sq_to_pos = |sq: &str| -> egui::Pos2 {
        let col_char = sq.chars().nth(0).unwrap();
        let row_char = sq.chars().nth(1).unwrap();

        let col = col_char as u32 - 'a' as u32;
        let row = row_char.to_digit(10).unwrap_or(1) - 1;

        let cell_w = rect.width() / 8.0;
        let cell_h = rect.height() / 8.0;

        egui::pos2(
            rect.min.x + (col as f32 + 0.5) * cell_w,
            rect.min.y + (7.0 - row as f32 + 0.5) * cell_h,
        )
    };

    let start = sq_to_pos(from_sq);
    let end = sq_to_pos(to_sq);

    painter.line_segment([start, end], egui::Stroke::new(5.0, color));

    // Draw arrowhead
    let dir = (end - start).normalized();
    if dir.length() > 0.0 {
        let norm = egui::vec2(-dir.y, dir.x);
        let arrow_head_size = 15.0;
        let p1 = end - dir * arrow_head_size + norm * arrow_head_size * 0.5;
        let p2 = end - dir * arrow_head_size - norm * arrow_head_size * 0.5;

        painter.add(egui::Shape::convex_polygon(
            vec![end, p1, p2],
            color,
            egui::Stroke::NONE,
        ));
    }
}
