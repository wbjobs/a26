use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchMatch {
    pub timestamp_ms: u64,
    pub score: f32,
    pub thumbnail_b64: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrameFeature {
    pub timestamp_ms: u64,
    pub vector: Vec<f32>,
}

pub struct ClipSearch {
    index: Arc<Mutex<HashMap<String, Vec<FrameFeature>>>>,
}

fn l2_normalize(v: &mut [f32]) {
    let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 1e-8 {
        for x in v.iter_mut() {
            *x /= norm;
        }
    }
}

fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b.iter()).map(|(x, y)| x * y).sum::<f32>()
}

fn pseudo_image_feature(rgba: &[u8], width: u32, height: u32) -> Vec<f32> {
    let grid = 8u32;
    let cell_w = width.max(1) / grid;
    let cell_h = height.max(1) / grid;
    let mut feat = Vec::with_capacity((grid * grid * 3) as usize);

    for gy in 0..grid {
        for gx in 0..grid {
            let mut r_sum = 0u64;
            let mut g_sum = 0u64;
            let mut b_sum = 0u64;
            let mut count = 0u64;

            let start_y = gy * cell_h;
            let end_y = (start_y + cell_h).min(height);
            let start_x = gx * cell_w;
            let end_x = (start_x + cell_w).min(width);

            for y in start_y..end_y {
                for x in start_x..end_x {
                    let idx = ((y * width + x) * 4) as usize;
                    if idx + 2 < rgba.len() {
                        r_sum += rgba[idx] as u64;
                        g_sum += rgba[idx + 1] as u64;
                        b_sum += rgba[idx + 2] as u64;
                        count += 1;
                    }
                }
            }

            if count > 0 {
                feat.push((r_sum / count) as f32 / 255.0);
                feat.push((g_sum / count) as f32 / 255.0);
                feat.push((b_sum / count) as f32 / 255.0);
            } else {
                feat.push(0.0);
                feat.push(0.0);
                feat.push(0.0);
            }
        }
    }

    l2_normalize(&mut feat);
    feat
}

fn pseudo_text_feature(text: &str) -> Vec<f32> {
    let dim = 192usize;
    let mut feat = vec![0.0f32; dim];
    let bytes = text.as_bytes();

    for (i, &b) in bytes.iter().enumerate() {
        let val = b as f32 / 255.0;
        let idx0 = (i * 3) % dim;
        let idx1 = (i * 3 + 7) % dim;
        let idx2 = (i * 5 + 11) % dim;
        feat[idx0] += val * 0.6;
        feat[idx1] += val * 0.3;
        feat[idx2] += val * 0.1;
    }

    let keywords: &[(&str, [usize; 6])] = &[
        ("chrome", [0, 1, 2, 3, 4, 5]),
        ("firefox", [6, 7, 8, 9, 10, 11]),
        ("browser", [0, 6, 12, 13, 14, 15]),
        ("打开", [16, 17, 18, 19, 20, 21]),
        ("close", [22, 23, 24, 25, 26, 27]),
        ("关闭", [22, 23, 24, 25, 26, 27]),
        ("游戏", [28, 29, 30, 31, 32, 33]),
        ("game", [28, 29, 30, 31, 32, 33]),
        ("play", [28, 29, 34, 35, 36, 37]),
        ("editor", [38, 39, 40, 41, 42, 43]),
        ("vscode", [38, 39, 40, 44, 45, 46]),
        ("代码", [38, 39, 40, 41, 42, 43]),
        ("terminal", [47, 48, 49, 50, 51, 52]),
        ("命令行", [47, 48, 49, 50, 51, 52]),
        ("file", [53, 54, 55, 56, 57, 58]),
        ("文件", [53, 54, 55, 56, 57, 58]),
        ("click", [59, 60, 61, 62, 63, 64]),
        ("点击", [59, 60, 61, 62, 63, 64]),
        ("scroll", [65, 66, 67, 68, 69, 70]),
        ("滚动", [65, 66, 67, 68, 69, 70]),
    ];

    let text_lower = text.to_lowercase();
    for (kw, indices) in keywords {
        if text_lower.contains(kw) {
            for &i in indices.iter() {
                if i < feat.len() {
                    feat[i] += 0.4;
                }
            }
        }
    }

    l2_normalize(&mut feat);
    feat
}

fn pad_or_trim(v: Vec<f32>, target_len: usize) -> Vec<f32> {
    if v.len() == target_len {
        return v;
    }
    let mut result = vec![0.0f32; target_len];
    let copy_len = v.len().min(target_len);
    result[..copy_len].copy_from_slice(&v[..copy_len]);
    l2_normalize(&mut result);
    result
}

impl ClipSearch {
    pub fn new() -> Self {
        Self {
            index: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn index_frames(
        &self,
        recording_id: &str,
        frames: &[(u64, Vec<u8>, u32, u32)],
    ) -> Result<usize> {
        let mut index = self.index.lock().unwrap();
        let mut features = Vec::with_capacity(frames.len());

        for (ts, rgba, w, h) in frames {
            let mut vec = pseudo_image_feature(rgba, *w, *h);
            vec = pad_or_trim(vec, 192);
            features.push(FrameFeature {
                timestamp_ms: *ts,
                vector: vec,
            });
        }

        let count = features.len();
        index.insert(recording_id.to_string(), features);
        Ok(count)
    }

    pub fn is_indexed(&self, recording_id: &str) -> bool {
        let index = self.index.lock().unwrap();
        index.contains_key(recording_id)
    }

    pub fn search(
        &self,
        recording_id: &str,
        query: &str,
        top_k: usize,
    ) -> Result<Vec<SearchMatch>> {
        let index = self.index.lock().unwrap();
        let features = index
            .get(recording_id)
            .with_context(|| format!("Recording {} not indexed", recording_id))?;

        let text_vec = pseudo_text_feature(query);
        let text_vec = pad_or_trim(text_vec, 192);

        let mut scores: Vec<(u64, f32)> = features
            .iter()
            .map(|f| (f.timestamp_ms, cosine_similarity(&f.vector, &text_vec)))
            .collect();

        scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        Ok(scores
            .into_iter()
            .take(top_k)
            .map(|(ts, score)| SearchMatch {
                timestamp_ms: ts,
                score,
                thumbnail_b64: None,
            })
            .collect())
    }
}

impl Default for ClipSearch {
    fn default() -> Self {
        Self::new()
    }
}
