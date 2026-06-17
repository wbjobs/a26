pub mod capture;
pub mod compressor;
pub mod commands;
pub mod diff;
pub mod power_monitor;
pub mod recorder;
pub mod storage;

use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use tauri::Manager;

use recorder::Recorder;
use storage::Database;

pub struct AppState {
    pub recorder: Mutex<Option<Recorder>>,
    pub db: Mutex<Option<Database>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
    pub logical_x: f64,
    pub logical_y: f64,
    pub logical_width: f64,
    pub logical_height: f64,
}

impl DiffRect {
    pub fn with_scale(x: u32, y: u32, width: u32, height: u32, scale_factor: f64) -> Self {
        Self {
            x,
            y,
            width,
            height,
            logical_x: x as f64 / scale_factor,
            logical_y: y as f64 / scale_factor,
            logical_width: width as f64 / scale_factor,
            logical_height: height as f64 / scale_factor,
        }
    }

    pub fn fill_logical(&mut self, scale_factor: f64) {
        self.logical_x = self.x as f64 / scale_factor;
        self.logical_y = self.y as f64 / scale_factor;
        self.logical_width = self.width as f64 / scale_factor;
        self.logical_height = self.height as f64 / scale_factor;
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrameDiff {
    pub timestamp_ms: u64,
    pub rects: Vec<DiffRect>,
    pub blocks_data: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Recording {
    pub id: String,
    pub title: String,
    pub start_time: i64,
    pub end_time: Option<i64>,
    pub duration_ms: u64,
    pub width: u32,
    pub height: u32,
    pub logical_width: f64,
    pub logical_height: f64,
    pub scale_factor: f64,
    pub frame_count: u32,
    pub thumbnail: Option<String>,
    pub total_size: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnnotationItem {
    pub id: String,
    #[serde(rename = "type")]
    pub item_type: String,
    pub x: f64,
    pub y: f64,
    pub width: Option<f64>,
    pub height: Option<f64>,
    pub end_x: Option<f64>,
    pub end_y: Option<f64>,
    pub text: Option<String>,
    pub color: String,
    pub stroke_width: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnnotationLayer {
    pub timestamp_ms: u64,
    pub items: Vec<AnnotationItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrameInfo {
    pub timestamp_ms: u64,
    pub rects: Vec<DiffRect>,
    pub block_images: Vec<String>,
    pub recording_width: u32,
    pub recording_height: u32,
    pub recording_logical_width: f64,
    pub recording_logical_height: f64,
    pub scale_factor: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct RecordingStatus {
    pub status: String,
    pub recording_id: Option<String>,
    pub title: Option<String>,
    pub elapsed_ms: u64,
    pub frame_count: u32,
    pub auto_paused_reason: Option<String>,
}

#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}!", name)
}

pub fn run() {
    let db_path = dirs::data_local_dir()
        .map(|p| p.join("pixel-recorder").join("recordings.db"))
        .unwrap_or_else(|| std::path::PathBuf::from("./recordings.db"));

    tauri::Builder::default()
        .manage(AppState {
            recorder: Mutex::new(None),
            db: Mutex::new(None),
        })
        .setup(move |app| {
            let state = app.state::<AppState>();
            let mut db_guard = state.db.lock().unwrap();
            *db_guard = Some(Database::new(db_path).expect("Failed to initialize database"));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            greet,
            commands::start_recording,
            commands::stop_recording,
            commands::pause_recording,
            commands::resume_recording,
            commands::list_recordings,
            commands::get_recording,
            commands::get_frames_range,
            commands::save_annotations,
            commands::get_annotations,
            commands::delete_recording,
            commands::search_recordings,
            commands::get_recording_status,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
