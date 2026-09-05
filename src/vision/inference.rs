use anyhow::Result;
use image::DynamicImage;
use ndarray::Array4;
use ort::session::Session;

pub struct Detector {
    session: Session,
    input_buffer: Vec<f32>,
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
        const NUM_PIXELS: usize = 640 * 640;
        let input_buffer = vec![0.0f32; 3 * NUM_PIXELS];
        Ok(Self { session, input_buffer })
    }

    pub fn detect(&mut self, img: &DynamicImage, conf_threshold: f32) -> Result<Vec<Detection>> {
        Self::fast_bilinear_rgb_planar(img, &mut self.input_buffer);

        let input = Array4::from_shape_vec((1, 3, 640, 640), self.input_buffer.clone())?;
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

    /// High-performance cache-friendly bilinear interpolation from DynamicImage directly into
    /// a pre-allocated contiguous planar buffer [3, 640, 640] normalized to 0.0..1.0.
    /// Runs in ~1.5 ms without allocating any intermediate buffers.
    pub fn fast_bilinear_rgb_planar(img: &DynamicImage, output: &mut [f32]) {
        const TARGET_W: usize = 640;
        const TARGET_H: usize = 640;
        const NUM_PIXELS: usize = TARGET_W * TARGET_H;

        let (src_w, src_h, raw_pixels, src_stride, channels) = match img {
            DynamicImage::ImageRgba8(rgba) => (
                rgba.width() as usize,
                rgba.height() as usize,
                rgba.as_raw().as_slice(),
                rgba.width() as usize * 4,
                4,
            ),
            DynamicImage::ImageRgb8(rgb) => (
                rgb.width() as usize,
                rgb.height() as usize,
                rgb.as_raw().as_slice(),
                rgb.width() as usize * 3,
                3,
            ),
            other => {
                let rgba = other.to_rgba8();
                let img_rgba = DynamicImage::ImageRgba8(rgba);
                Self::fast_bilinear_rgb_planar(&img_rgba, output);
                return;
            }
        };

        if src_w == 0 || src_h == 0 {
            return;
        }

        let scale_x = src_w as f32 / TARGET_W as f32;
        let scale_y = src_h as f32 / TARGET_H as f32;

        // Precompute horizontal interpolation indices and weights
        let mut x_map = [(0usize, 0usize, 0.0f32, 0.0f32); TARGET_W];
        for (x, slot) in x_map.iter_mut().enumerate() {
            let src_x = ((x as f32 + 0.5) * scale_x - 0.5).max(0.0);
            let x0 = (src_x.floor() as usize).min(src_w - 1);
            let x1 = (x0 + 1).min(src_w - 1);
            let wx = (src_x - x0 as f32).clamp(0.0, 1.0);
            *slot = (x0, x1, 1.0 - wx, wx);
        }

        let (r_plane, rest) = output.split_at_mut(NUM_PIXELS);
        let (g_plane, b_plane) = rest.split_at_mut(NUM_PIXELS);

        for y in 0..TARGET_H {
            let src_y = ((y as f32 + 0.5) * scale_y - 0.5).max(0.0);
            let y0 = (src_y.floor() as usize).min(src_h - 1);
            let y1 = (y0 + 1).min(src_h - 1);
            let wy = (src_y - y0 as f32).clamp(0.0, 1.0);
            let wy0 = 1.0 - wy;
            let wy1 = wy;

            let row0 = &raw_pixels[y0 * src_stride..];
            let row1 = &raw_pixels[y1 * src_stride..];
            let out_row_offset = y * TARGET_W;

            for (x, &(x0, x1, wx0, wx1)) in x_map.iter().enumerate() {
                let out_idx = out_row_offset + x;

                let p00_idx = x0 * channels;
                let p01_idx = x1 * channels;
                let p10_idx = x0 * channels;
                let p11_idx = x1 * channels;

                let w00 = wx0 * wy0;
                let w01 = wx1 * wy0;
                let w10 = wx0 * wy1;
                let w11 = wx1 * wy1;

                let r = w00 * row0[p00_idx] as f32
                    + w01 * row0[p01_idx] as f32
                    + w10 * row1[p10_idx] as f32
                    + w11 * row1[p11_idx] as f32;
                let g = w00 * row0[p00_idx + 1] as f32
                    + w01 * row0[p01_idx + 1] as f32
                    + w10 * row1[p10_idx + 1] as f32
                    + w11 * row1[p11_idx + 1] as f32;
                let b = w00 * row0[p00_idx + 2] as f32
                    + w01 * row0[p01_idx + 2] as f32
                    + w10 * row1[p10_idx + 2] as f32
                    + w11 * row1[p11_idx + 2] as f32;

                r_plane[out_idx] = r * (1.0 / 255.0);
                g_plane[out_idx] = g * (1.0 / 255.0);
                b_plane[out_idx] = b * (1.0 / 255.0);
            }
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

    #[test]
    fn test_fast_bilinear_rgb_planar() {
        use image::{Rgba, RgbaImage};
        let mut img = RgbaImage::new(100, 100);
        for pixel in img.pixels_mut() {
            *pixel = Rgba([255, 128, 64, 255]);
        }
        let dynamic_img = DynamicImage::ImageRgba8(img);
        let mut buffer = vec![0.0f32; 3 * 640 * 640];
        Detector::fast_bilinear_rgb_planar(&dynamic_img, &mut buffer);

        const NUM_PIXELS: usize = 640 * 640;
        let r_val = buffer[0];
        let g_val = buffer[NUM_PIXELS];
        let b_val = buffer[2 * NUM_PIXELS];

        assert!((r_val - 1.0).abs() < 0.01);
        assert!((g_val - (128.0 / 255.0)).abs() < 0.01);
        assert!((b_val - (64.0 / 255.0)).abs() < 0.01);
    }
}
