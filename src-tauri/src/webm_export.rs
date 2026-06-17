use anyhow::{Context, Result};
use byteorder::{LittleEndian, WriteBytesExt};
use image::RgbaImage;
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use crate::compressor::Compressor;
use crate::storage::Database;
use crate::FrameInfo;

#[derive(Debug, Clone)]
pub struct ExportProgress {
    pub total_frames: u32,
    pub rendered_frames: u32,
    pub current_ms: u64,
    pub bytes_written: u64,
    pub finished: bool,
}

#[derive(Clone)]
pub struct WebMExporter {
    progress: Arc<Mutex<Option<ExportProgress>>>,
}

fn ebml_varint(value: u64) -> Vec<u8> {
    if value < 0x7F {
        return vec![(value as u8) | 0x80];
    }
    if value < 0x3FFF {
        return vec![((value >> 8) as u8) | 0x40, (value & 0xFF) as u8];
    }
    if value < 0x1FFFFF {
        return vec![
            ((value >> 16) as u8) | 0x20,
            ((value >> 8) & 0xFF) as u8,
            (value & 0xFF) as u8,
        ];
    }
    if value < 0xFFFFFFF {
        return vec![
            ((value >> 24) as u8) | 0x10,
            ((value >> 16) & 0xFF) as u8,
            ((value >> 8) & 0xFF) as u8,
            (value & 0xFF) as u8,
        ];
    }
    let mut buf = vec![0x08];
    for i in (0..56).step_by(8).rev() {
        buf.push(((value >> i) & 0xFF) as u8);
    }
    buf
}

fn ebml_element(id: &[u8], data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(id.len() + 8 + data.len());
    out.extend_from_slice(id);
    out.extend_from_slice(&ebml_varint(data.len() as u64));
    out.extend_from_slice(data);
    out
}

fn ebml_uint(id: &[u8], value: u64) -> Vec<u8> {
    let mut data = Vec::new();
    let mut v = value;
    let mut bytes = 0u8;
    loop {
        data.push((v & 0xFF) as u8);
        v >>= 8;
        bytes += 1;
        if v == 0 || bytes >= 8 {
            break;
        }
    }
    data.reverse();
    ebml_element(id, &data)
}

fn ebml_float(id: &[u8], value: f64) -> Vec<u8> {
    let mut data = Vec::new();
    data.write_f64::<LittleEndian>(value).unwrap();
    ebml_element(id, &data)
}

fn ebml_string(id: &[u8], s: &str) -> Vec<u8> {
    ebml_element(id, s.as_bytes())
}

struct WebMWriter<W: Write> {
    w: W,
    width: u32,
    height: u32,
    fps: f64,
    timecode_scale: u64,
    cluster_timecode: u64,
    cluster_buffer: Vec<u8>,
    frame_count: u32,
    total_duration_ns: u64,
    segment_size_pos: Option<u64>,
}

impl<W: Write> WebMWriter<W> {
    fn new(mut w: W, width: u32, height: u32, fps: f64) -> Result<Self> {
        let timecode_scale = 1_000_000u64;

        let mut ebml_header = Vec::new();
        ebml_header.extend_from_slice(&ebml_element(b"\x1a\x45\xdf\xa3", {
            let mut inner = Vec::new();
            inner.extend_from_slice(&ebml_uint(b"\x42\x86", 1));
            inner.extend_from_slice(&ebml_uint(b"\x42\xf7", 1));
            inner.extend_from_slice(&ebml_uint(b"\x42\xf2", 4));
            inner.extend_from_slice(&ebml_uint(b"\x42\xf3", 8));
            inner.extend_from_slice(&ebml_string(b"\x42\x82", "matroska"));
            inner
        }));
        w.write_all(&ebml_header)?;

        let segment_id = b"\x18\x53\x80\x67";
        w.write_all(segment_id)?;
        let size_pos = w.stream_position().ok();
        w.write_all(&ebml_varint(0x0FFFFFFFFFFFFFFF))?;

        let mut info = Vec::new();
        info.extend_from_slice(&ebml_uint(b"\x2a\xd7\xb1", timecode_scale));
        info.extend_from_slice(&ebml_string(b"\x4d\x80", "PixelRecorder"));
        info.extend_from_slice(&ebml_string(b"\x57\x41", "PixelRecorder"));
        w.write_all(&ebml_element(b"\x15\x49\xa9\x66", &info))?;

        let mut track = Vec::new();
        track.extend_from_slice(&ebml_uint(b"\xd7", 1));
        track.extend_from_slice(&ebml_uint(b"\x83", 1));
        track.extend_from_slice(&ebml_string(b"\x86", "V_VP8"));
        track.extend_from_slice(&ebml_string(b"\x22\xb5\x9c", "Video"));
        track.extend_from_slice(&ebml_element(b"\xe0", {
            let mut video = Vec::new();
            video.extend_from_slice(&ebml_uint(b"\xb0", width as u64));
            video.extend_from_slice(&ebml_uint(b"\xba", height as u64));
            video.extend_from_slice(&ebml_float(b"\x23\x83\xe3", fps));
            video
        }));
        let tracks = ebml_element(b"\x16\x54\xae\x6b", &track);
        w.write_all(&tracks)?;

        Ok(Self {
            w,
            width,
            height,
            fps,
            timecode_scale,
            cluster_timecode: 0,
            cluster_buffer: Vec::new(),
            frame_count: 0,
            total_duration_ns: 0,
            segment_size_pos: size_pos,
        })
    }

    fn start_cluster(&mut self, timecode_ms: u64) -> Result<()> {
        self.cluster_timecode = timecode_ms;
        self.cluster_buffer.clear();
        self.cluster_buffer.extend_from_slice(&ebml_uint(
            b"\xe7",
            (timecode_ms * 1_000_000) / self.timecode_scale,
        ));
        Ok(())
    }

    fn add_frame(&mut self, rgba: &[u8], timestamp_ms: u64) -> Result<()> {
        let pixel_count = (self.width * self.height) as usize;
        if self.cluster_buffer.is_empty() {
            self.start_cluster(timestamp_ms)?;
        }

        let mut block = Vec::new();
        block.write_u8(0x81).unwrap();
        let relative = (timestamp_ms.saturating_sub(self.cluster_timecode)).min(32767) as i16;
        block.write_i16::<LittleEndian>(relative).unwrap();
        block.write_u8(0x00).unwrap();

        let mut y_plane = Vec::with_capacity(pixel_count);
        for i in 0..pixel_count {
            let idx = i * 4;
            let r = rgba[idx] as u32;
            let g = rgba[idx + 1] as u32;
            let b = rgba[idx + 2] as u32;
            let y = ((66 * r + 129 * g + 25 * b + 128) >> 8) as u8 + 16;
            y_plane.push(y);
        }

        let half_w = (self.width / 2) as usize;
        let half_h = (self.height / 2) as usize;
        let mut u_plane = Vec::with_capacity(half_w * half_h);
        let mut v_plane = Vec::with_capacity(half_w * half_h);
        for row in (0..self.height).step_by(2) {
            for col in (0..self.width).step_by(2) {
                let idx = ((row * self.width + col) * 4) as usize;
                let r = rgba[idx] as u32;
                let g = rgba[idx + 1] as u32;
                let b = rgba[idx + 2] as u32;
                let u = ((-38 * r - 74 * g + 112 * b + 128) >> 8) as u8 + 128;
                let v = ((112 * r - 94 * g - 18 * b + 128) >> 8) as u8 + 128;
                u_plane.push(u);
                v_plane.push(v);
            }
        }

        let mut raw_frame = Vec::new();
        raw_frame.push(0x10);
        raw_frame.extend_from_slice(&y_plane);
        raw_frame.extend_from_slice(&u_plane);
        raw_frame.extend_from_slice(&v_plane);

        block.extend_from_slice(&raw_frame);

        self.cluster_buffer
            .extend_from_slice(&ebml_element(b"\xa3", &block));

        self.frame_count += 1;
        self.total_duration_ns = (timestamp_ms as u64) * 1_000_000;

        if self.frame_count % 60 == 0 {
            self.flush_cluster()?;
        }
        Ok(())
    }

    fn flush_cluster(&mut self) -> Result<()> {
        if !self.cluster_buffer.is_empty() {
            let cluster = ebml_element(b"\x1f\x43\xb6\x75", &self.cluster_buffer);
            self.w.write_all(&cluster)?;
            self.cluster_buffer.clear();
        }
        Ok(())
    }

    fn finish(mut self) -> Result<(u32, u64)> {
        self.flush_cluster()?;
        let frames = self.frame_count;
        let bytes = self.w.stream_position().unwrap_or(0);
        Ok((frames, bytes))
    }
}

fn decode_frame_block(
    b64_images: &[String],
    rects: &[crate::DiffRect],
    canvas: &mut RgbaImage,
) {
    for (i, rect) in rects.iter().enumerate() {
        if i >= b64_images.len() {
            break;
        }
        let raw = match base64_decode(&b64_images[i]) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let block_img = match image::load_from_memory(&raw) {
            Ok(img) => img.to_rgba8(),
            Err(_) => continue,
        };
        let bw = block_img.width().min(rect.width);
        let bh = block_img.height().min(rect.height);
        let bx = rect.x.min(canvas.width().saturating_sub(bw));
        let by = rect.y.min(canvas.height().saturating_sub(bh));
        for dy in 0..bh {
            for dx in 0..bw {
                let src_idx = ((dy * bw + dx) * 4) as usize;
                let dst_x = bx + dx;
                let dst_y = by + dy;
                if dst_x < canvas.width() && dst_y < canvas.height() && src_idx + 3 < block_img.as_raw().len() {
                    let dst_idx = ((dst_y * canvas.width() + dst_x) * 4) as usize;
                    let raw = canvas.as_raw_mut();
                    if dst_idx + 3 < raw.len() && src_idx + 3 < block_img.as_raw().len() {
                        raw[dst_idx] = block_img.as_raw()[src_idx];
                        raw[dst_idx + 1] = block_img.as_raw()[src_idx + 1];
                        raw[dst_idx + 2] = block_img.as_raw()[src_idx + 2];
                        raw[dst_idx + 3] = block_img.as_raw()[src_idx + 3];
                    }
                }
            }
        }
    }
}

fn base64_decode(s: &str) -> Result<Vec<u8>> {
    use base64::prelude::*;
    BASE64_STANDARD.decode(s).context("base64 decode failed")
}

fn decode_diff_blocks(
    zstd_data: &[u8],
) -> Result<Vec<Vec<u8>>> {
    if zstd_data.is_empty() {
        return Ok(Vec::new());
    }
    Compressor::decompress_blocks(zstd_data).context("decompress failed")
}

impl WebMExporter {
    pub fn new() -> Self {
        Self {
            progress: Arc::new(Mutex::new(None)),
        }
    }

    pub fn get_progress(&self) -> Option<ExportProgress> {
        self.progress.lock().unwrap().clone()
    }

    pub fn export(
        &self,
        db: &Database,
        recording_id: &str,
        output_path: PathBuf,
    ) -> Result<(u32, u64)> {
        let rec = db
            .get_recording(recording_id)?
            .with_context(|| format!("Recording {} not found", recording_id))?;

        let total_frames = rec.frame_count;
        *self.progress.lock().unwrap() = Some(ExportProgress {
            total_frames,
            rendered_frames: 0,
            current_ms: 0,
            bytes_written: 0,
            finished: false,
        });

        let frames = db.get_frames_in_range(recording_id, 0, rec.duration_ms)?;

        let file = std::fs::File::create(&output_path)
            .with_context(|| format!("Failed to create {}", output_path.display()))?;
        let writer = std::io::BufWriter::new(file);
        let mut webm = WebMWriter::new(writer, rec.width, rec.height, 10.0)?;

        let mut canvas = RgbaImage::new(rec.width, rec.height);

        if let Some(thumb_b64) = rec.thumbnail.as_ref() {
            if let Ok(raw) = base64_decode(thumb_b64) {
                if let Ok(img) = image::load_from_memory(&raw) {
                    let resized = image::imageops::resize(
                        &img,
                        rec.width,
                        rec.height,
                        image::imageops::FilterType::Triangle,
                    );
                    canvas = resized.to_rgba8();
                }
            }
        }

        let mut rendered = 0u32;

        for frame in &frames {
            decode_frame_block_from_db(&frame, db, recording_id, &mut canvas)?;
            webm.add_frame(canvas.as_raw(), frame.timestamp_ms)?;
            rendered += 1;

            *self.progress.lock().unwrap() = Some(ExportProgress {
                total_frames,
                rendered_frames: rendered,
                current_ms: frame.timestamp_ms,
                bytes_written: 0,
                finished: false,
            });
        }

        let (count, bytes) = webm.finish()?;

        *self.progress.lock().unwrap() = Some(ExportProgress {
            total_frames,
            rendered_frames: count,
            current_ms: rec.duration_ms,
            bytes_written: bytes,
            finished: true,
        });

        Ok((count, bytes))
    }
}

fn decode_frame_block_from_db(
    info: &FrameInfo,
    db: &Database,
    _recording_id: &str,
    canvas: &mut RgbaImage,
) -> Result<()> {
    let b64_images = &info.block_images;
    let rects = &info.rects;
    decode_frame_block(b64_images, rects, canvas);
    let _ = db;
    Ok(())
}

impl Default for WebMExporter {
    fn default() -> Self {
        Self::new()
    }
}
