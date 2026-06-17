use anyhow::Result;
use tauri::{command, State};

use crate::recorder::Recorder;
use crate::storage::Database;
use crate::{AnnotationLayer, AppState, FrameInfo, Recording, RecordingStatus};

#[derive(serde::Serialize)]
pub struct CommandResult<T> {
    pub success: bool,
    pub data: Option<T>,
    pub error: Option<String>,
}

impl<T> CommandResult<T> {
    fn ok(data: T) -> Self {
        Self {
            success: true,
            data: Some(data),
            error: None,
        }
    }

    fn err(msg: impl Into<String>) -> Self {
        Self {
            success: false,
            data: None,
            error: Some(msg.into()),
        }
    }
}

fn with_db<F, T>(state: &State<'_, AppState>, f: F) -> CommandResult<T>
where
    F: FnOnce(&Database) -> Result<T>,
{
    let guard = match state.db.lock() {
        Ok(g) => g,
        Err(e) => return CommandResult::err(format!("DB lock poisoned: {}", e)),
    };
    let db = match guard.as_ref() {
        Some(db) => db,
        None => return CommandResult::err("Database not initialized"),
    };
    match f(db) {
        Ok(v) => CommandResult::ok(v),
        Err(e) => CommandResult::err(format!("{}", e)),
    }
}

#[command]
pub fn start_recording(
    state: State<'_, AppState>,
    title: Option<String>,
) -> CommandResult<Recording> {
    let db_clone = {
        let guard = match state.db.lock() {
            Ok(g) => g,
            Err(e) => return CommandResult::err(format!("DB lock poisoned: {}", e)),
        };
        match guard.as_ref() {
            Some(db) => db.clone(),
            None => return CommandResult::err("Database not initialized"),
        }
    };

    {
        let mut guard = match state.recorder.lock() {
            Ok(g) => g,
            Err(e) => return CommandResult::err(format!("Recorder lock poisoned: {}", e)),
        };

        if guard.is_some() {
            return CommandResult::err("Recorder already active");
        }

        let recorder = Recorder::new(db_clone);
        match recorder.start(title) {
            Ok(rec) => {
                *guard = Some(recorder);
                CommandResult::ok(rec)
            }
            Err(e) => CommandResult::err(format!("{}", e)),
        }
    }
}

#[command]
pub fn stop_recording(state: State<'_, AppState>) -> CommandResult<Recording> {
    let mut recorder_guard = match state.recorder.lock() {
        Ok(g) => g,
        Err(e) => return CommandResult::err(format!("Recorder lock poisoned: {}", e)),
    };

    let recorder = match recorder_guard.as_ref() {
        Some(r) => r,
        None => return CommandResult::err("No active recording"),
    };

    match recorder.stop() {
        Ok(rec) => {
            recorder_guard.take();
            CommandResult::ok(rec)
        }
        Err(e) => CommandResult::err(format!("{}", e)),
    }
}

#[command]
pub fn pause_recording(state: State<'_, AppState>) -> CommandResult<String> {
    let recorder_guard = match state.recorder.lock() {
        Ok(g) => g,
        Err(e) => return CommandResult::err(format!("Recorder lock poisoned: {}", e)),
    };

    let recorder = match recorder_guard.as_ref() {
        Some(r) => r,
        None => return CommandResult::err("No active recording"),
    };

    match recorder.pause() {
        Ok(()) => CommandResult::ok("paused".to_string()),
        Err(e) => CommandResult::err(format!("{}", e)),
    }
}

#[command]
pub fn resume_recording(state: State<'_, AppState>) -> CommandResult<String> {
    let recorder_guard = match state.recorder.lock() {
        Ok(g) => g,
        Err(e) => return CommandResult::err(format!("Recorder lock poisoned: {}", e)),
    };

    let recorder = match recorder_guard.as_ref() {
        Some(r) => r,
        None => return CommandResult::err("No active recording"),
    };

    match recorder.resume() {
        Ok(()) => CommandResult::ok("resumed".to_string()),
        Err(e) => CommandResult::err(format!("{}", e)),
    }
}

#[command]
pub fn list_recordings(state: State<'_, AppState>) -> CommandResult<Vec<Recording>> {
    with_db(&state, |db| db.list_recordings())
}

#[command]
pub fn get_recording(
    state: State<'_, AppState>,
    recording_id: String,
) -> CommandResult<Option<Recording>> {
    with_db(&state, |db| db.get_recording(&recording_id))
}

#[command]
pub fn get_frames_range(
    state: State<'_, AppState>,
    recording_id: String,
    start_ms: u64,
    end_ms: u64,
) -> CommandResult<Vec<FrameInfo>> {
    with_db(&state, |db| {
        db.get_frames_in_range(&recording_id, start_ms, end_ms)
    })
}

#[command]
pub fn save_annotations(
    state: State<'_, AppState>,
    recording_id: String,
    layers: Vec<AnnotationLayer>,
) -> CommandResult<String> {
    with_db(&state, |db| {
        for layer in &layers {
            db.save_annotation(&recording_id, layer)?;
        }
        Ok("saved".to_string())
    })
}

#[command]
pub fn get_annotations(
    state: State<'_, AppState>,
    recording_id: String,
) -> CommandResult<Vec<AnnotationLayer>> {
    with_db(&state, |db| db.get_annotations(&recording_id))
}

#[command]
pub fn delete_recording(
    state: State<'_, AppState>,
    recording_id: String,
) -> CommandResult<String> {
    with_db(&state, |db| {
        db.delete_recording(&recording_id)?;
        Ok("deleted".to_string())
    })
}

#[command]
pub fn search_recordings(
    state: State<'_, AppState>,
    title_query: Option<String>,
    date_query: Option<String>,
) -> CommandResult<Vec<Recording>> {
    with_db(&state, |db| {
        db.search_recordings(title_query.as_deref(), date_query.as_deref())
    })
}

#[command]
pub fn get_recording_status(state: State<'_, AppState>) -> CommandResult<RecordingStatus> {
    let recorder_guard = match state.recorder.lock() {
        Ok(g) => g,
        Err(e) => return CommandResult::err(format!("Recorder lock poisoned: {}", e)),
    };

    let recorder = match recorder_guard.as_ref() {
        Some(r) => r,
        None => {
            return CommandResult::ok(RecordingStatus {
                status: "idle".to_string(),
                recording_id: None,
                title: None,
                elapsed_ms: 0,
                frame_count: 0,
                auto_paused_reason: None,
            })
        }
    };

    let state_val = recorder.state();
    let status_str = match state_val {
        crate::recorder::RecorderState::Idle => "idle",
        crate::recorder::RecorderState::Recording => "recording",
        crate::recorder::RecorderState::Paused => "paused",
    };

    let recording_id = recorder.current_recording_id();
    let (title, elapsed_ms, frame_count, auto_paused_reason) = recorder.current_status_info();

    let status_str = if auto_paused_reason.is_some() {
        "auto_paused".to_string()
    } else {
        status_str.to_string()
    };

    CommandResult::ok(RecordingStatus {
        status: status_str,
        recording_id,
        title,
        elapsed_ms,
        frame_count,
        auto_paused_reason,
    })
}
