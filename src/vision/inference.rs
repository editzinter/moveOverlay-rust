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

        // Try CUDA (maximum speed for NVIDIA GPUs), fall back to CPU if it fails
        let session = match Session::builder()?
            .with_execution_providers([
                ort::execution_providers::CUDAExecutionProvider::default().build()
            ])?
            .commit_from_file(model_path)
        {
            Ok(s) => {
                println!("CUDA execution provider loaded successfully!");
                s
            }
            Err(e) => {
                println!("CUDA failed: {:?}, falling back to CPU...", e);
                Session::builder()?.commit_from_file(model_path)?
            }
        };

        println!("ONNX Session created successfully");
        Ok(Self { session })
    }

    pub fn detect(&mut self, img: &DynamicImage, conf_threshold: f32) -> Result<Vec<Detection>> {
        let resized = img.resize_exact(640, 640, FilterType::Triangle);
        let rgb = resized.to_rgb8();

        let mut input = Array4::<f32>::zeros((1, 3, 640, 640));
        for (x, y, pixel) in rgb.enumerate_pixels() {
            input[[0, 0, y as usize, x as usize]] = pixel[0] as f32 / 255.0;
            input[[0, 1, y as usize, x as usize]] = pixel[1] as f32 / 255.0;
            input[[0, 2, y as usize, x as usize]] = pixel[2] as f32 / 255.0;
        }

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
                    let x = data[0 * num_boxes + i];
                    let y = data[1 * num_boxes + i];
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

        Ok(self.nms(detections))
    }

    fn nms(&self, mut detections: Vec<Detection>) -> Vec<Detection> {
        detections.sort_by(|a, b| b.confidence.partial_cmp(&a.confidence).unwrap());
        let mut result = Vec::new();
        while !detections.is_empty() {
            let best = detections.remove(0);
            detections.retain(|d| self.iou(&best.bbox, &d.bbox) < 0.45);
            result.push(best);
        }
        result
    }

    fn iou(&self, box1: &[f32; 4], box2: &[f32; 4]) -> f32 {
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
        let area1 = box1[2] * box1[3];
        let area2 = box2[2] * box2[3];
        intersection / (area1 + area2 - intersection)
    }
}
