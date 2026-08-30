use anyhow::Result;
use image::{imageops::FilterType, DynamicImage};
use ndarray::Array4;
use ort::session::Session;

pub struct Detector {
    session: Session,
}

#[derive(Debug, Clone)]
pub struct Detection {
    pub class_id: usize,
    pub confidence: f32,
    pub bbox: [f32; 4], // x, y, w, h (normalized)
}

impl Detector {
    pub fn new(model_path: &str) -> Result<Self> {
        println!("Attempting to create ONNX session with CUDA (NVIDIA)...");

        // Try CUDA (maximum speed for NVIDIA GPUs)
        let cuda_session = Session::builder()
            .and_then(|b| {
                b.with_execution_providers([
                    ort::execution_providers::CUDAExecutionProvider::default().build(),
                ])
            })
            .and_then(|b| b.commit_from_file(model_path));

        let session = match cuda_session {
            Ok(s) => {
                println!("CUDA execution provider loaded successfully!");
                s
            }
            Err(e) => {
                println!("CUDA unavailable ({:?}), trying CPU fallback...", e);
                Session::builder()?.commit_from_file(model_path)?
            }
        };

        println!("ONNX Session created successfully");
        Ok(Self { session })
    }

    pub fn detect(&mut self, img: &DynamicImage, conf_threshold: f32) -> Result<Vec<Detection>> {
        let resized = img.resize_exact(640, 640, FilterType::Triangle);
        let rgb = resized.to_rgb8();

        // High-performance contiguous planar memory copy (eliminates >1.2M 4D index ops/frame)
        let raw = rgb.as_raw();
        const NUM_PIXELS: usize = 640 * 640;
        let mut input_data = vec![0.0f32; 3 * NUM_PIXELS];
        let (r_plane, rest) = input_data.split_at_mut(NUM_PIXELS);
        let (g_plane, b_plane) = rest.split_at_mut(NUM_PIXELS);

        for i in 0..NUM_PIXELS {
            let offset = i * 3;
            r_plane[i] = raw[offset] as f32 / 255.0;
            g_plane[i] = raw[offset + 1] as f32 / 255.0;
            b_plane[i] = raw[offset + 2] as f32 / 255.0;
        }

        let input = Array4::from_shape_vec((1, 3, 640, 640), input_data)?;
        let input_tensor = ort::value::Tensor::from_array(input)?;
        let mut detections = Vec::new();

        {
            let outputs = self.session.run(ort::inputs!["images" => input_tensor])?;
            let output_tensor = outputs["output0"].try_extract_tensor::<f32>()?;
            let (_shape, data) = output_tensor;

            let num_classes = 13;
            let num_boxes = 8400;

            for i in 0..num_boxes {
                let mut max_conf = 0.0;
                let mut class_id = 0;

                for c in 0..num_classes {
                    let idx = (4 + c) * num_boxes + i;
                    let conf = data[idx];
                    if conf > max_conf {
                        max_conf = conf;
                        class_id = c;
                    }
                }

                if max_conf > conf_threshold {
                    let x = data[i];
                    let y = data[num_boxes + i];
                    let w = data[2 * num_boxes + i];
                    let h = data[3 * num_boxes + i];

                    detections.push(Detection {
                        class_id,
                        confidence: max_conf,
                        bbox: [x, y, w, h],
                    });
                }
            }
        }

        Ok(Self::nms(detections))
    }

    pub fn nms(mut detections: Vec<Detection>) -> Vec<Detection> {
        detections.sort_unstable_by(|a, b| {
            b.confidence
                .partial_cmp(&a.confidence)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        let mut result = Vec::with_capacity(detections.len().min(64));
        let mut suppressed = vec![false; detections.len()];

        for i in 0..detections.len() {
            if suppressed[i] {
                continue;
            }
            let current = &detections[i];
            result.push(current.clone());
            for j in (i + 1)..detections.len() {
                if !suppressed[j] {
                    // Suppress if both are board boxes or both are piece boxes with IoU >= 0.45
                    let both_boards = current.class_id == 0 && detections[j].class_id == 0;
                    let both_pieces = current.class_id != 0 && detections[j].class_id != 0;
                    if (both_boards || both_pieces)
                        && Self::iou(&current.bbox, &detections[j].bbox) >= 0.45
                    {
                        suppressed[j] = true;
                    }
                }
            }
        }
        result
    }

    pub fn iou(box1: &[f32; 4], box2: &[f32; 4]) -> f32 {
        let b1_x1 = box1[0] - box1[2] / 2.0;
        let b1_y1 = box1[1] - box1[3] / 2.0;
        let b1_x2 = box1[0] + box1[2] / 2.0;
        let b1_y2 = box1[1] + box1[3] / 2.0;

        let b2_x1 = box2[0] - box2[2] / 2.0;
        let b2_y1 = box2[1] - box2[3] / 2.0;
        let b2_x2 = box2[0] + box2[2] / 2.0;
        let b2_y2 = box2[1] + box2[3] / 2.0;

        let x1 = b1_x1.max(b2_x1);
        let y1 = b1_y1.max(b2_y1);
        let x2 = b1_x2.min(b2_x2);
        let y2 = b1_y2.min(b2_y2);

        let intersection = (x2 - x1).max(0.0) * (y2 - y1).max(0.0);
        let area1 = (box1[2] * box1[3]).max(0.0);
        let area2 = (box2[2] * box2[3]).max(0.0);
        let union = area1 + area2 - intersection;

        if union <= 1e-6 {
            0.0
        } else {
            intersection / union
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_iou_calculation() {
        let b1 = [0.5, 0.5, 0.2, 0.2];
        let b2 = [0.5, 0.5, 0.2, 0.2];
        // Identical boxes -> IoU ~ 1.0
        let iou_same = Detector::iou(&b1, &b2);
        assert!((iou_same - 1.0).abs() < 1e-4);

        // Disjoint boxes -> IoU = 0.0
        let b3 = [0.1, 0.1, 0.1, 0.1];
        let b4 = [0.9, 0.9, 0.1, 0.1];
        let iou_disjoint = Detector::iou(&b3, &b4);
        assert_eq!(iou_disjoint, 0.0);

        // Zero area box -> no NaN
        let b_zero = [0.5, 0.5, 0.0, 0.0];
        let iou_zero = Detector::iou(&b1, &b_zero);
        assert!(!iou_zero.is_nan());
    }

    #[test]
    fn test_nms_suppression() {
        let d1 = Detection {
            class_id: 1,
            confidence: 0.9,
            bbox: [0.5, 0.5, 0.2, 0.2],
        };
        let d2 = Detection {
            class_id: 1,
            confidence: 0.8,
            bbox: [0.51, 0.51, 0.2, 0.2], // Highly overlapping with d1
        };
        let d3 = Detection {
            class_id: 2,
            confidence: 0.85,
            bbox: [0.1, 0.1, 0.1, 0.1], // Separate box
        };

        let result = Detector::nms(vec![d1, d2, d3]);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].confidence, 0.9);
        assert_eq!(result[1].confidence, 0.85);
    }
}
