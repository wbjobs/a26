use anyhow::{Context, Result};
use image::{ImageBuffer, Rgba, RgbaImage};
use opencv::core::{Mat, MatTraitConst, Vector, CV_8UC1, CV_8UC3};
use opencv::imgproc::{
    bounding_rect, find_contours, contour_area, morphology_default_kernel, RETR_EXTERNAL, CHAIN_APPROX_SIMPLE, MORPH_OPEN,
};
use opencv::video::{BackgroundSubtractorMOG2, create_background_subtractor_mog2};
use std::io::Cursor;

use crate::{compressor::Compressor, DiffRect, FrameDiff};

const BLOCK_SIZE: u32 = 32;
const PIXEL_DIFF_THRESHOLD: u8 = 25;
const MIN_CONTOUR_AREA: f64 = 100.0;
const ROI_PADDING: i32 = 16;
const MAX_ROI_COUNT: usize = 30;

pub struct DiffEngine {
    block_size: u32,
    threshold: u8,
    bg_subtractor: BackgroundSubtractorMOG2,
    initialized: bool,
    enable_opencv: bool,
}

impl Default for DiffEngine {
    fn default() -> Self {
        let bg_subtractor = create_background_subtractor_mog2(
            500,
            16.0,
            true,
        )
        .expect("Failed to create MOG2 background subtractor");
        Self {
            block_size: BLOCK_SIZE,
            threshold: PIXEL_DIFF_THRESHOLD,
            bg_subtractor,
            initialized: false,
            enable_opencv: true,
        }
    }
}

impl DiffEngine {
    pub fn new(block_size: u32, threshold: u8) -> Self {
        let bg_subtractor = create_background_subtractor_mog2(
            500,
            16.0,
            true,
        )
        .expect("Failed to create MOG2 background subtractor");
        Self {
            block_size,
            threshold,
            bg_subtractor,
            initialized: false,
            enable_opencv: true,
        }
    }

    pub fn with_opencv_disabled() -> Self {
        let bg_subtractor = create_background_subtractor_mog2(500, 16.0, true)
            .expect("Failed to create MOG2 background subtractor");
        Self {
            block_size: BLOCK_SIZE,
            threshold: PIXEL_DIFF_THRESHOLD,
            bg_subtractor,
            initialized: false,
            enable_opencv: false,
        }
    }

    pub fn compute_diff(
        &mut self,
        prev: &RgbaImage,
        current: &RgbaImage,
        timestamp_ms: u64,
    ) -> Result<FrameDiff> {
        if self.enable_opencv {
            self.compute_diff_opencv(prev, current, timestamp_ms)
        } else {
            self.compute_diff_fallback(prev, current, timestamp_ms)
        }
    }

    fn compute_diff_opencv(
        &mut self,
        prev: &RgbaImage,
        current: &RgbaImage,
        timestamp_ms: u64,
    ) -> Result<FrameDiff> {
        let (width, height) = current.dimensions();
        let current_mat = rgba_image_to_mat_bgr(current)?;

        let mut fg_mask = Mat::default();
        self.bg_subtractor
            .apply(&current_mat, &mut fg_mask, -1.0)
            .context("MOG2 apply failed")?;

        if !self.initialized {
            self.initialized = true;
            return Ok(FrameDiff {
                timestamp_ms,
                rects: Vec::new(),
                blocks_data: Vec::new(),
            });
        }

        let mut cleaned = Mat::default();
        let kernel = morphology_default_kernel();
        opencv::imgproc::morphology_ex(&fg_mask, &mut cleaned, MORPH_OPEN, &kernel, opencv::core::Point::new(-1, -1), 1, opencv::core::BORDER_CONSTANT, opencv::core::Scalar::default())
            .context("Morphology open failed")?;

        let mut contours: Vector<Vector<opencv::core::Point>> = Vector::new();
        let mut hierarchy = Mat::default();
        find_contours(
            &cleaned,
            &mut contours,
            &mut hierarchy,
            RETR_EXTERNAL,
            CHAIN_APPROX_SIMPLE,
            opencv::core::Point::new(0, 0),
        )
        .context("findContours failed")?;

        let mut motion_rois: Vec<opencv::core::Rect> = Vec::new();
        for i in 0..contours.len() {
            let contour = contours.get(i)?;
            let area = contour_area(&contour, false)?;
            if area < MIN_CONTOUR_AREA {
                continue;
            }
            let mut rect = bounding_rect(&contour)?;
            rect.x = (rect.x - ROI_PADDING).max(0);
            rect.y = (rect.y - ROI_PADDING).max(0);
            rect.width = (rect.width + 2 * ROI_PADDING).min((width as i32 - rect.x).max(0));
            rect.height = (rect.height + 2 * ROI_PADDING).min((height as i32 - rect.y).max(0));
            if rect.width > 0 && rect.height > 0 {
                motion_rois.push(rect);
            }
        }

        let merged_rois = merge_overlapping_rois(motion_rois, width, height);

        if merged_rois.is_empty() {
            return Ok(FrameDiff {
                timestamp_ms,
                rects: Vec::new(),
                blocks_data: Vec::new(),
            });
        }

        let mut all_changed_blocks = Vec::new();
        let cols = (width + self.block_size - 1) / self.block_size;

        for roi in &merged_rois {
            let start_col = (roi.x as u32 / self.block_size).max(0);
            let start_row = (roi.y as u32 / self.block_size).max(0);
            let end_col = ((roi.x as u32 + roi.width as u32 + self.block_size - 1) / self.block_size).min(cols);
            let rows = (height + self.block_size - 1) / self.block_size;
            let end_row = ((roi.y as u32 + roi.height as u32 + self.block_size - 1) / self.block_size).min(rows);

            for row in start_row..end_row {
                for col in start_col..end_col {
                    let bx = col * self.block_size;
                    let by = row * self.block_size;
                    let block_w = self.block_size.min(width - bx);
                    let block_h = self.block_size.min(height - by);
                    if self.is_block_changed(prev, current, bx, by, block_w, block_h) {
                        all_changed_blocks.push((col, row));
                    }
                }
            }
        }

        if all_changed_blocks.is_empty() {
            return Ok(FrameDiff {
                timestamp_ms,
                rects: Vec::new(),
                blocks_data: Vec::new(),
            });
        }

        let rects = merge_blocks_into_rects(&all_changed_blocks, self.block_size, width, height);
        let blocks = self.crop_and_encode_blocks(current, &rects)?;
        let blocks_data = Compressor::compress_multiple(&blocks)?;

        Ok(FrameDiff {
            timestamp_ms,
            rects,
            blocks_data,
        })
    }

    fn compute_diff_fallback(
        &self,
        prev: &RgbaImage,
        current: &RgbaImage,
        timestamp_ms: u64,
    ) -> Result<FrameDiff> {
        let (width, height) = current.dimensions();
        let cols = (width + self.block_size - 1) / self.block_size;
        let rows = (height + self.block_size - 1) / self.block_size;
        let changed_blocks = self.find_changed_blocks(prev, current, cols, rows);

        if changed_blocks.is_empty() {
            return Ok(FrameDiff {
                timestamp_ms,
                rects: Vec::new(),
                blocks_data: Vec::new(),
            });
        }

        let rects = merge_blocks_into_rects(&changed_blocks, self.block_size, width, height);
        let blocks = self.crop_and_encode_blocks(current, &rects)?;
        let blocks_data = Compressor::compress_multiple(&blocks)?;

        Ok(FrameDiff {
            timestamp_ms,
            rects,
            blocks_data,
        })
    }

    fn find_changed_blocks(
        &self,
        prev: &RgbaImage,
        current: &RgbaImage,
        cols: u32,
        rows: u32,
    ) -> Vec<(u32, u32)> {
        let mut changed = Vec::new();
        let (width, height) = current.dimensions();

        for row in 0..rows {
            for col in 0..cols {
                let bx = col * self.block_size;
                let by = row * self.block_size;
                let block_w = self.block_size.min(width - bx);
                let block_h = self.block_size.min(height - by);
                if self.is_block_changed(prev, current, bx, by, block_w, block_h) {
                    changed.push((col, row));
                }
            }
        }

        changed
    }

    fn is_block_changed(
        &self,
        prev: &RgbaImage,
        current: &RgbaImage,
        bx: u32,
        by: u32,
        w: u32,
        h: u32,
    ) -> bool {
        let step_x = (w / 4).max(1);
        let step_y = (h / 4).max(1);
        for y in (by..by + h).step_by(step_y as usize) {
            for x in (bx..bx + w).step_by(step_x as usize) {
                let p_prev = prev.get_pixel(x, y);
                let p_curr = current.get_pixel(x, y);
                if pixel_diff(p_prev, p_curr) > self.threshold {
                    return true;
                }
            }
        }

        for y in by..by + h {
            for x in bx..bx + w {
                let p_prev = prev.get_pixel(x, y);
                let p_curr = current.get_pixel(x, y);
                if pixel_diff(p_prev, p_curr) > self.threshold {
                    return true;
                }
            }
        }
        false
    }

    fn crop_and_encode_blocks(
        &self,
        image: &RgbaImage,
        rects: &[DiffRect],
    ) -> Result<Vec<Vec<u8>>> {
        let mut blocks = Vec::with_capacity(rects.len());
        for rect in rects {
            let cropped = crop_image(image, rect.x, rect.y, rect.width, rect.height);
            let encoded = encode_png(&cropped)?;
            blocks.push(encoded);
        }
        Ok(blocks)
    }

    pub fn encode_thumbnail(image: &RgbaImage, max_size: u32) -> Result<Vec<u8>> {
        let (w, h) = image.dimensions();
        let scale = (max_size as f64 / w.max(h) as f64).min(1.0);
        let new_w = (w as f64 * scale) as u32;
        let new_h = (h as f64 * scale) as u32;
        let resized = image::imageops::resize(image, new_w, new_h, image::imageops::FilterType::Triangle);
        encode_png(&resized)
    }

    pub fn reset_background(&mut self) {
        self.bg_subtractor = create_background_subtractor_mog2(500, 16.0, true)
            .expect("Failed to recreate MOG2");
        self.initialized = false;
    }
}

fn rgba_image_to_mat_bgr(image: &RgbaImage) -> Result<Mat> {
    let (w, h) = image.dimensions();
    let mut bgr_data = Vec::with_capacity((w * h * 3) as usize);
    for pixel in image.pixels() {
        bgr_data.push(pixel[2]);
        bgr_data.push(pixel[1]);
        bgr_data.push(pixel[0]);
    }
    let mut mat = Mat::new_rows_cols(h as i32, w as i32, CV_8UC3)?;
    mat.data_bytes_mut()?.copy_from_slice(&bgr_data);
    Ok(mat)
}

fn merge_overlapping_rois(
    rois: Vec<opencv::core::Rect>,
    img_w: u32,
    img_h: u32,
) -> Vec<opencv::core::Rect> {
    if rois.is_empty() {
        return Vec::new();
    }

    let mut sorted = rois;
    sorted.sort_by_key(|r| (r.x, r.y));

    let mut merged: Vec<opencv::core::Rect> = Vec::new();
    let mut current = sorted[0];

    for roi in sorted.iter().skip(1) {
        let expanded = opencv::core::Rect {
            x: (current.x - ROI_PADDING).max(0),
            y: (current.y - ROI_PADDING).max(0),
            width: (current.width + 2 * ROI_PADDING).min((img_w as i32 - current.x).max(0)),
            height: (current.height + 2 * ROI_PADDING).min((img_h as i32 - current.y).max(0)),
        };

        if rects_overlap(&expanded, roi) {
            current = union_rects(&current, roi, img_w, img_h);
        } else {
            merged.push(current);
            current = *roi;
        }
    }
    merged.push(current);

    if merged.len() > MAX_ROI_COUNT {
        merged.sort_by(|a, b| (b.width * b.height).cmp(&(a.width * a.height)));
        merged.truncate(MAX_ROI_COUNT);
    }

    merged
}

fn rects_overlap(a: &opencv::core::Rect, b: &opencv::core::Rect) -> bool {
    !(a.x + a.width < b.x || b.x + b.width < a.x || a.y + a.height < b.y || b.y + b.height < a.y)
}

fn union_rects(a: &opencv::core::Rect, b: &opencv::core::Rect, max_w: u32, max_h: u32) -> opencv::core::Rect {
    let x = a.x.min(b.x);
    let y = a.y.min(b.y);
    let right = (a.x + a.width).max(b.x + b.width);
    let bottom = (a.y + a.height).max(b.y + b.height);
    opencv::core::Rect {
        x,
        y,
        width: (right - x).min(max_w as i32),
        height: (bottom - y).min(max_h as i32),
    }
}

fn pixel_diff(a: &Rgba<u8>, b: &Rgba<u8>) -> u8 {
    let dr = (a[0] as i32 - b[0] as i32).unsigned_abs() as u32;
    let dg = (a[1] as i32 - b[1] as i32).unsigned_abs() as u32;
    let db = (a[2] as i32 - b[2] as i32).unsigned_abs() as u32;
    ((dr + dg + db) / 3) as u8
}

fn merge_blocks_into_rects(blocks: &[(u32, u32)], block_size: u32, _img_w: u32, _img_h: u32) -> Vec<DiffRect> {
    if blocks.is_empty() {
        return Vec::new();
    }

    let mut visited = std::collections::HashSet::new();
    let block_set: std::collections::HashSet<_> = blocks.iter().collect();
    let mut rects = Vec::new();

    for &block in blocks {
        if visited.contains(&block) {
            continue;
        }

        let mut queue = vec![block];
        let mut min_c = block.0;
        let mut max_c = block.0;
        let mut min_r = block.1;
        let mut max_r = block.1;

        visited.insert(block);

        while let Some((c, r)) = queue.pop() {
            min_c = min_c.min(c);
            max_c = max_c.max(c);
            min_r = min_r.min(r);
            max_r = max_r.max(r);

            let neighbors = [
                (c + 1, r),
                (c.saturating_sub(1), r),
                (c, r + 1),
                (c, r.saturating_sub(1)),
            ];

            for &n in &neighbors {
                if block_set.contains(&n) && !visited.contains(&n) {
                    visited.insert(n);
                    queue.push(n);
                }
            }
        }

        let x = min_c * block_size;
        let y = min_r * block_size;
        let width = (max_c - min_c + 1) * block_size;
        let height = (max_r - min_r + 1) * block_size;

        rects.push(DiffRect {
            x,
            y,
            width,
            height,
            logical_x: 0.0,
            logical_y: 0.0,
            logical_width: 0.0,
            logical_height: 0.0,
        });
    }

    rects
}

fn crop_image(src: &RgbaImage, x: u32, y: u32, w: u32, h: u32) -> RgbaImage {
    let (sw, sh) = src.dimensions();
    let x = x.min(sw);
    let y = y.min(sh);
    let w = w.min(sw.saturating_sub(x));
    let h = h.min(sh.saturating_sub(y));

    let mut out = ImageBuffer::new(w, h);
    for oy in 0..h {
        for ox in 0..w {
            let p = src.get_pixel(x + ox, y + oy);
            out.put_pixel(ox, oy, *p);
        }
    }
    out
}

fn encode_png(image: &RgbaImage) -> Result<Vec<u8>> {
    let mut buffer = Vec::new();
    {
        let mut cursor = Cursor::new(&mut buffer);
        let encoder = image::codecs::png::PngEncoder::new(&mut cursor);
        use image::ImageEncoder;
        encoder
            .write_image(
                image.as_raw(),
                image.width(),
                image.height(),
                image::ColorType::Rgba8,
            )
            .context("PNG encoding failed")?;
    }
    Ok(buffer)
}
