use ndarray::ArrayView3;
use std::cmp::Ordering;

#[derive(Debug, Clone, Copy)]
pub struct Detection {
    pub x: f32, // center x
    pub y: f32, // center y
    pub w: f32,
    pub h: f32,
    pub class_id: usize,
    pub confidence: f32,
}

impl Detection {
    // Returns (x1, y1, x2, y2)
    pub fn bounds(&self) -> (f32, f32, f32, f32) {
        let half_w = self.w / 2.0;
        let half_h = self.h / 2.0;
        (
            self.x - half_w,
            self.y - half_h,
            self.x + half_w,
            self.y + half_h,
        )
    }
}

pub fn postprocess(
    output: ArrayView3<f32>,
    conf_threshold: f32,
    iou_threshold: f32,
) -> Vec<Detection> {
    let mut detections = Vec::new();

    // Output shape is [Batch, Channels, Anchors] -> [1, 17, 8400]
    // Channels: 0=x, 1=y, 2=w, 3=h, 4..16=classes
    let rows = output.shape()[1]; // 17
    let cols = output.shape()[2]; // 8400

    for i in 0..cols {
        // Find the class with maximum score
        let mut max_score = 0.0;
        let mut class_id = 0;

        // Class scores start at index 4
        for c in 4..rows {
            let score = output[[0, c, i]];
            if score > max_score {
                max_score = score;
                class_id = c - 4; // Map back to 0-12
            }
        }

        if max_score > conf_threshold {
            let x = output[[0, 0, i]];
            let y = output[[0, 1, i]];
            let w = output[[0, 2, i]];
            let h = output[[0, 3, i]];

            detections.push(Detection {
                x,
                y,
                w,
                h,
                class_id,
                confidence: max_score,
            });
        }
    }

    non_maximum_suppression(detections, iou_threshold)
}

fn non_maximum_suppression(mut detections: Vec<Detection>, iou_threshold: f32) -> Vec<Detection> {
    detections.sort_by(|a, b| {
        b.confidence
            .partial_cmp(&a.confidence)
            .unwrap_or(Ordering::Equal)
    });

    let mut keep = Vec::new();
    let mut active = vec![true; detections.len()];

    for i in 0..detections.len() {
        if !active[i] {
            continue;
        }
        keep.push(detections[i]);

        for j in (i + 1)..detections.len() {
            if active[j] {
                let iou = calculate_iou(&detections[i], &detections[j]);
                if iou > iou_threshold {
                    active[j] = false;
                }
            }
        }
    }
    keep
}

fn calculate_iou(a: &Detection, b: &Detection) -> f32 {
    let (ax1, ay1, ax2, ay2) = a.bounds();
    let (bx1, by1, bx2, by2) = b.bounds();

    let inter_x1 = ax1.max(bx1);
    let inter_y1 = ay1.max(by1);
    let inter_x2 = ax2.min(bx2);
    let inter_y2 = ay2.min(by2);

    if inter_x2 < inter_x1 || inter_y2 < inter_y1 {
        return 0.0;
    }

    let inter_area = (inter_x2 - inter_x1) * (inter_y2 - inter_y1);
    let a_area = a.w * a.h;
    let b_area = b.w * b.h;

    inter_area / (a_area + b_area - inter_area)
}
