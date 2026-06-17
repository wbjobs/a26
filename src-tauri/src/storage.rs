use anyhow::{Context, Result};
use base64::prelude::*;
use image::RgbaImage;
use rusqlite::{params, Connection, OptionalExtension};
use std::io::Cursor;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use crate::compressor::Compressor;
use crate::{AnnotationLayer, DiffRect, FrameDiff, FrameInfo, Recording};

#[derive(Clone)]
pub struct Database {
    conn: Arc<Mutex<Connection>>,
}

impl Database {
    pub fn new(path: PathBuf) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).context("Failed to create database directory")?;
        }
        let conn = Connection::open(&path).context("Failed to open database")?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")
            .context("Failed to set pragmas")?;
        let db = Self {
            conn: Arc::new(Mutex::new(conn)),
        };
        db.init_schema()?;
        Ok(db)
    }

    fn init_schema(&self) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute_batch(
            "
                CREATE TABLE IF NOT EXISTS recordings (
                    id TEXT PRIMARY KEY,
                    title TEXT NOT NULL,
                    start_time INTEGER NOT NULL,
                    end_time INTEGER,
                    duration_ms INTEGER NOT NULL DEFAULT 0,
                    width INTEGER NOT NULL,
                    height INTEGER NOT NULL,
                    frame_count INTEGER NOT NULL DEFAULT 0,
                    thumbnail TEXT
                );

                CREATE TABLE IF NOT EXISTS frames (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    recording_id TEXT NOT NULL,
                    timestamp_ms INTEGER NOT NULL,
                    diff_rects_json TEXT NOT NULL,
                    zstd_png_blocks BLOB NOT NULL,
                    FOREIGN KEY (recording_id) REFERENCES recordings(id) ON DELETE CASCADE
                );

                CREATE INDEX IF NOT EXISTS idx_frames_recording ON frames(recording_id);
                CREATE INDEX IF NOT EXISTS idx_frames_timestamp ON frames(timestamp_ms);

                CREATE TABLE IF NOT EXISTS annotations (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    recording_id TEXT NOT NULL,
                    timestamp_ms INTEGER NOT NULL,
                    layer_json TEXT NOT NULL,
                    FOREIGN KEY (recording_id) REFERENCES recordings(id) ON DELETE CASCADE
                );

                CREATE INDEX IF NOT EXISTS idx_annotations_recording ON annotations(recording_id);
                CREATE INDEX IF NOT EXISTS idx_annotations_timestamp ON annotations(timestamp_ms);
            ",
        )
        .context("Failed to initialize database schema")?;

        conn.execute_batch(
            "
                ALTER TABLE recordings ADD COLUMN IF NOT EXISTS logical_width REAL NOT NULL DEFAULT 0;
                ALTER TABLE recordings ADD COLUMN IF NOT EXISTS logical_height REAL NOT NULL DEFAULT 0;
                ALTER TABLE recordings ADD COLUMN IF NOT EXISTS scale_factor REAL NOT NULL DEFAULT 1.0;
            ",
        )
        .context("Failed to migrate recordings schema")?;

        Ok(())
    }

    pub fn insert_recording(&self, recording: &Recording) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO recordings (id, title, start_time, end_time, duration_ms, width, height, frame_count, thumbnail, logical_width, logical_height, scale_factor)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![
                recording.id,
                recording.title,
                recording.start_time,
                recording.end_time,
                recording.duration_ms as i64,
                recording.width as i64,
                recording.height as i64,
                recording.frame_count as i64,
                recording.thumbnail,
                recording.logical_width,
                recording.logical_height,
                recording.scale_factor,
            ],
        )
        .context("Failed to insert recording")?;
        Ok(())
    }

    pub fn update_recording_end(
        &self,
        id: &str,
        end_time: i64,
        duration_ms: u64,
        frame_count: u32,
    ) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE recordings SET end_time = ?1, duration_ms = ?2, frame_count = ?3 WHERE id = ?4",
            params![end_time, duration_ms as i64, frame_count as i64, id],
        )
        .context("Failed to update recording end")?;
        Ok(())
    }

    pub fn insert_frames_batch(&self, recording_id: &str, frames: &[FrameDiff]) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let tx = conn.unchecked_transaction().context("Failed to start transaction")?;
        {
            let mut stmt = tx
                .prepare(
                    "INSERT INTO frames (recording_id, timestamp_ms, diff_rects_json, zstd_png_blocks)
                     VALUES (?1, ?2, ?3, ?4)",
                )
                .context("Failed to prepare frame insert statement")?;

            for frame in frames {
                let rects_json =
                    serde_json::to_string(&frame.rects).context("Failed to serialize rects")?;
                stmt.execute(params![
                    recording_id,
                    frame.timestamp_ms as i64,
                    rects_json,
                    frame.blocks_data,
                ])
                .context("Failed to insert frame")?;
            }
        }
        tx.commit().context("Failed to commit frame batch")?;
        Ok(())
    }

    fn get_total_size(&self, conn: &Connection, recording_id: &str) -> Result<Option<i64>> {
        let mut stmt = conn
            .prepare("SELECT COALESCE(SUM(LENGTH(zstd_png_blocks)), 0) FROM frames WHERE recording_id = ?1")
            .context("Failed to prepare total size statement")?;
        let size: i64 = stmt
            .query_row(params![recording_id], |row| row.get(0))
            .context("Failed to query total size")?;
        Ok(Some(size))
    }

    pub fn list_recordings(&self) -> Result<Vec<Recording>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT id, title, start_time, end_time, duration_ms, width, height, frame_count, thumbnail, logical_width, logical_height, scale_factor
                 FROM recordings ORDER BY start_time DESC",
            )
            .context("Failed to prepare list recordings statement")?;

        let rows = stmt
            .query_map([], Self::map_recording_row)
            .context("Failed to query recordings")?;

        let mut recordings = Vec::new();
        for row in rows {
            let mut rec = row.context("Failed to read recording row")?;
            rec.total_size = self.get_total_size(&conn, &rec.id)?;
            recordings.push(rec);
        }
        Ok(recordings)
    }

    pub fn search_recordings(
        &self,
        title_query: Option<&str>,
        date_query: Option<&str>,
    ) -> Result<Vec<Recording>> {
        let conn = self.conn.lock().unwrap();

        let mut sql = String::from(
            "SELECT id, title, start_time, end_time, duration_ms, width, height, frame_count, thumbnail, logical_width, logical_height, scale_factor
             FROM recordings WHERE 1=1",
        );
        let mut params_vec: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

        if let Some(tq) = title_query {
            sql.push_str(" AND title LIKE ?");
            params_vec.push(Box::new(format!("%{}%", tq)));
        }

        if let Some(dq) = date_query {
            sql.push_str(" AND date(start_time, 'unixepoch') = ?");
            params_vec.push(Box::new(dq.to_string()));
        }

        sql.push_str(" ORDER BY start_time DESC");

        let mut stmt = conn
            .prepare(&sql)
            .context("Failed to prepare search statement")?;

        let param_refs: Vec<&dyn rusqlite::ToSql> = params_vec.iter().map(|p| p.as_ref()).collect();

        let rows = stmt
            .query_map(param_refs.as_slice(), Self::map_recording_row)
            .context("Failed to search recordings")?;

        let mut recordings = Vec::new();
        for row in rows {
            let mut rec = row.context("Failed to read search row")?;
            rec.total_size = self.get_total_size(&conn, &rec.id)?;
            recordings.push(rec);
        }
        Ok(recordings)
    }

    pub fn get_recording(&self, id: &str) -> Result<Option<Recording>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT id, title, start_time, end_time, duration_ms, width, height, frame_count, thumbnail, logical_width, logical_height, scale_factor
                 FROM recordings WHERE id = ?1",
            )
            .context("Failed to prepare get recording statement")?;

        let recording = stmt
            .query_row(params![id], Self::map_recording_row)
            .optional()
            .context("Failed to get recording")?;

        if let Some(mut rec) = recording {
            rec.total_size = self.get_total_size(&conn, &rec.id)?;
            Ok(Some(rec))
        } else {
            Ok(None)
        }
    }

    fn raw_png_to_base64(raw_data: &[u8]) -> Result<String> {
        let img: RgbaImage =
            image::load_from_memory(raw_data).context("Failed to load raw PNG data")?.to_rgba8();
        let mut buffer = Vec::new();
        {
            let mut cursor = Cursor::new(&mut buffer);
            let encoder = image::codecs::png::PngEncoder::new(&mut cursor);
            use image::ImageEncoder;
            encoder
                .write_image(
                    img.as_raw(),
                    img.width(),
                    img.height(),
                    image::ColorType::Rgba8,
                )
                .context("PNG re-encoding failed")?;
        }
        Ok(BASE64_STANDARD.encode(&buffer))
    }

    pub fn get_frames_in_range(
        &self,
        recording_id: &str,
        start_ms: u64,
        end_ms: u64,
    ) -> Result<Vec<FrameInfo>> {
        let conn = self.conn.lock().unwrap();

        let (rec_width, rec_height, rec_logical_width, rec_logical_height, rec_scale_factor): (u32, u32, f64, f64, f64) = {
            let mut stmt = conn
                .prepare("SELECT width, height, logical_width, logical_height, scale_factor FROM recordings WHERE id = ?1")
                .context("Failed to prepare recording dimensions statement")?;
            stmt.query_row(params![recording_id], |row| {
                let w: i64 = row.get(0)?;
                let h: i64 = row.get(1)?;
                let lw: f64 = row.try_get(2).unwrap_or(0.0);
                let lh: f64 = row.try_get(3).unwrap_or(0.0);
                let sf: f64 = row.try_get(4).unwrap_or(1.0);
                Ok((w as u32, h as u32, lw, lh, sf))
            })
            .context("Failed to get recording dimensions")?
        };

        let mut stmt = conn
            .prepare(
                "SELECT timestamp_ms, diff_rects_json, zstd_png_blocks
                 FROM frames
                 WHERE recording_id = ?1 AND timestamp_ms BETWEEN ?2 AND ?3
                 ORDER BY timestamp_ms ASC",
            )
            .context("Failed to prepare frames range statement")?;

        let rows = stmt
            .query_map(
                params![recording_id, start_ms as i64, end_ms as i64],
                |row| {
                    let ts: i64 = row.get(0)?;
                    let rects_json: String = row.get(1)?;
                    let blocks_blob: Option<Vec<u8>> = row.get(2).ok();
                    let rects: Vec<DiffRect> = serde_json::from_str(&rects_json).unwrap_or_default();
                    Ok((ts as u64, rects, blocks_blob))
                },
            )
            .context("Failed to query frames")?;

        let mut frames = Vec::new();
        for row_result in rows {
            let (timestamp_ms, rects, blocks_blob) =
                row_result.context("Failed to read frame row")?;

            let block_images: Vec<String> = if let Some(blob) = blocks_blob {
                if blob.is_empty() {
                    Vec::new()
                } else {
                    match Compressor::decompress_blocks(&blob) {
                        Ok(blocks) => {
                            let mut images = Vec::with_capacity(blocks.len());
                            for block in blocks {
                                match Self::raw_png_to_base64(&block) {
                                    Ok(b64) => images.push(b64),
                                    Err(_) => images.push(String::new()),
                                }
                            }
                            images
                        }
                        Err(_) => Vec::new(),
                    }
                }
            } else {
                Vec::new()
            };

            frames.push(FrameInfo {
                timestamp_ms,
                rects,
                block_images,
                recording_width: rec_width,
                recording_height: rec_height,
                recording_logical_width: rec_logical_width,
                recording_logical_height: rec_logical_height,
                scale_factor: rec_scale_factor,
            });
        }
        Ok(frames)
    }

    pub fn save_annotation(&self, recording_id: &str, layer: &AnnotationLayer) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "DELETE FROM annotations WHERE recording_id = ?1 AND timestamp_ms = ?2",
            params![recording_id, layer.timestamp_ms as i64],
        )
        .context("Failed to delete old annotation")?;

        let layer_json =
            serde_json::to_string(&layer.items).context("Failed to serialize annotation layer")?;
        conn.execute(
            "INSERT INTO annotations (recording_id, timestamp_ms, layer_json)
             VALUES (?1, ?2, ?3)",
            params![recording_id, layer.timestamp_ms as i64, layer_json],
        )
        .context("Failed to insert annotation")?;
        Ok(())
    }

    pub fn get_annotations(&self, recording_id: &str) -> Result<Vec<AnnotationLayer>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT timestamp_ms, layer_json FROM annotations
                 WHERE recording_id = ?1 ORDER BY timestamp_ms ASC",
            )
            .context("Failed to prepare get annotations statement")?;

        let rows = stmt
            .query_map(params![recording_id], |row| {
                let ts: i64 = row.get(0)?;
                let layer_json: String = row.get(1)?;
                let items = serde_json::from_str(&layer_json).unwrap_or_default();
                Ok(AnnotationLayer {
                    timestamp_ms: ts as u64,
                    items,
                })
            })
            .context("Failed to query annotations")?;

        let mut layers = Vec::new();
        for row in rows {
            layers.push(row.context("Failed to read annotation row")?);
        }
        Ok(layers)
    }

    pub fn delete_recording(&self, id: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "DELETE FROM annotations WHERE recording_id = ?1",
            params![id],
        )
        .context("Failed to delete annotations")?;
        conn.execute(
            "DELETE FROM frames WHERE recording_id = ?1",
            params![id],
        )
        .context("Failed to delete frames")?;
        conn.execute(
            "DELETE FROM recordings WHERE id = ?1",
            params![id],
        )
        .context("Failed to delete recording")?;
        Ok(())
    }

    fn map_recording_row(row: &rusqlite::Row) -> rusqlite::Result<Recording> {
        let id: String = row.get(0)?;
        let title: String = row.get(1)?;
        let start_time: i64 = row.get(2)?;
        let end_time: Option<i64> = row.get(3)?;
        let duration_ms: i64 = row.get(4)?;
        let width: i64 = row.get(5)?;
        let height: i64 = row.get(6)?;
        let frame_count: i64 = row.get(7)?;
        let thumbnail: Option<String> = row.get(8)?;
        let logical_width: f64 = row.try_get(9).unwrap_or(0.0);
        let logical_height: f64 = row.try_get(10).unwrap_or(0.0);
        let scale_factor: f64 = row.try_get(11).unwrap_or(1.0);

        Ok(Recording {
            id,
            title,
            start_time,
            end_time,
            duration_ms: duration_ms as u64,
            width: width as u32,
            height: height as u32,
            logical_width,
            logical_height,
            scale_factor,
            frame_count: frame_count as u32,
            thumbnail,
            total_size: None,
        })
    }
}
