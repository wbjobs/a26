use anyhow::{Context, Result};
use base64::prelude::*;
use chrono::Utc;
use image::RgbaImage;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};
use uuid::Uuid;

use crate::capture::ScreenCapturer;
use crate::diff::DiffEngine;
use crate::power_monitor::{PowerEvent, PowerMonitor};
use crate::storage::Database;
use crate::{FrameDiff, Recording};

const BATCH_FLUSH_INTERVAL: Duration = Duration::from_secs(2);
const MAX_PENDING_FRAMES: usize = 100;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecorderState {
    Idle,
    Recording,
    Paused,
}

#[derive(Clone)]
struct SessionShared {
    inner: Arc<Mutex<SessionInner>>,
}

struct SessionInner {
    id: String,
    title: String,
    start_time: i64,
    start_instant: Instant,
    width: u32,
    height: u32,
    logical_width: f64,
    logical_height: f64,
    scale_factor: f64,
    frame_count: u32,
    elapsed_before_pause: u64,
    pause_started: Option<Instant>,
    auto_paused: bool,
    auto_paused_reason: Option<String>,
    pending_frames: Vec<FrameDiff>,
    previous_frame: Option<RgbaImage>,
}

impl SessionShared {
    fn new(
        width: u32,
        height: u32,
        logical_width: f64,
        logical_height: f64,
        scale_factor: f64,
        title: Option<String>,
    ) -> Self {
        let now = Utc::now();
        let ts = now.timestamp();
        let default_title = format!("Recording_{}", now.format("%Y%m%d_%H%M%S"));
        Self {
            inner: Arc::new(Mutex::new(SessionInner {
                id: Uuid::new_v4().to_string(),
                title: title.unwrap_or(default_title),
                start_time: ts,
                start_instant: Instant::now(),
                width,
                height,
                logical_width,
                logical_height,
                scale_factor,
                frame_count: 0,
                elapsed_before_pause: 0,
                pause_started: None,
                auto_paused: false,
                auto_paused_reason: None,
                pending_frames: Vec::new(),
                previous_frame: None,
            })),
        }
    }

    fn with<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut SessionInner) -> R,
    {
        let mut guard = self.inner.lock().unwrap();
        f(&mut guard)
    }

    fn read<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&SessionInner) -> R,
    {
        let guard = self.inner.lock().unwrap();
        f(&guard)
    }

    fn elapsed_ms(&self) -> u64 {
        self.read(|s| {
            let base = s.elapsed_before_pause;
            if s.is_paused() {
                base
            } else {
                base + s.start_instant.elapsed().as_millis() as u64
            }
        })
    }

    fn id(&self) -> String {
        self.read(|s| s.id.clone())
    }

    fn is_paused(&self) -> bool {
        self.read(|s| s.is_paused())
    }
}

impl SessionInner {
    fn is_paused(&self) -> bool {
        self.pause_started.is_some() || self.auto_paused
    }

    fn pause(&mut self) {
        if self.pause_started.is_none() && !self.auto_paused {
            self.elapsed_before_pause += self.start_instant.elapsed().as_millis() as u64;
            self.pause_started = Some(Instant::now());
        }
    }

    fn resume(&mut self) {
        if let Some(_) = self.pause_started.take() {
            self.start_instant = Instant::now();
        }
    }

    fn auto_pause(&mut self, reason: &str) {
        if !self.auto_paused {
            if self.pause_started.is_none() {
                self.elapsed_before_pause += self.start_instant.elapsed().as_millis() as u64;
                self.pause_started = Some(Instant::now());
            }
            self.auto_paused = true;
            self.auto_paused_reason = Some(reason.to_string());
        }
    }

    fn auto_resume(&mut self) {
        if self.auto_paused {
            self.auto_paused = false;
            self.auto_paused_reason = None;
            if self.pause_started.is_some() {
                self.pause_started = None;
                self.start_instant = Instant::now();
            }
        }
    }

    fn flush_pending(&mut self, db: &Database) -> Result<()> {
        if self.pending_frames.is_empty() {
            return Ok(());
        }
        let frames: Vec<FrameDiff> = self.pending_frames.drain(..).collect();
        let id = self.id.clone();
        db.insert_frames_batch(&id, &frames)
            .context("Failed to flush pending frames")?;
        Ok(())
    }
}

pub struct Recorder {
    state: Mutex<RecorderState>,
    session: Mutex<Option<SessionShared>>,
    capture_thread: Mutex<Option<JoinHandle<()>>>,
    flush_thread: Mutex<Option<JoinHandle<()>>>,
    running: Arc<AtomicBool>,
    power_monitor: Arc<Mutex<Option<PowerMonitor>>>,
    db: Database,
}

impl Recorder {
    pub fn new(db: Database) -> Self {
        Self {
            state: Mutex::new(RecorderState::Idle),
            session: Mutex::new(None),
            capture_thread: Mutex::new(None),
            flush_thread: Mutex::new(None),
            running: Arc::new(AtomicBool::new(false)),
            power_monitor: Arc::new(Mutex::new(None)),
            db,
        }
    }

    pub fn state(&self) -> RecorderState {
        *self.state.lock().unwrap()
    }

    pub fn current_recording_id(&self) -> Option<String> {
        self.session
            .lock()
            .unwrap()
            .as_ref()
            .map(|s| s.id())
    }

    pub fn current_status_info(&self) -> (Option<String>, u64, u32, Option<String>) {
        self.session
            .lock()
            .unwrap()
            .as_ref()
            .map(|s| {
                s.read(|inner| {
                    (
                        Some(inner.title.clone()),
                        s.elapsed_ms(),
                        inner.frame_count,
                        inner.auto_paused_reason.clone(),
                    )
                })
            })
            .unwrap_or((None, 0, 0, None))
    }

    pub fn start(&self, title: Option<String>) -> Result<Recording> {
        {
            let state = *self.state.lock().unwrap();
            if state != RecorderState::Idle {
                anyhow::bail!(
                    "Cannot start: recorder is not idle (state={:?})",
                    state
                );
            }
        }

        let capturer =
            ScreenCapturer::primary().context("Failed to create screen capturer")?;
        let (width, height) = capturer.dimensions();
        let scale_factor = capturer.scale_factor();
        let (logical_width, logical_height) = capturer.logical_dimensions();

        let first_frame = capturer
            .capture_frame()
            .context("Failed to capture first frame")?;
        let thumbnail = DiffEngine::encode_thumbnail(&first_frame, 320).ok();
        let thumbnail_b64 = thumbnail.map(|t| BASE64_STANDARD.encode(&t));

        let session = SessionShared::new(width, height, logical_width, logical_height, scale_factor, title);
        session.with(|s| {
            s.previous_frame = Some(first_frame.clone());
        });

        let recording = Recording {
            id: session.id(),
            title: session.read(|s| s.title.clone()),
            start_time: session.read(|s| s.start_time),
            end_time: None,
            duration_ms: 0,
            width,
            height,
            logical_width,
            logical_height,
            scale_factor,
            frame_count: 0,
            thumbnail: thumbnail_b64,
            total_size: None,
        };

        self.db
            .insert_recording(&recording)
            .context("Failed to insert recording into DB")?;

        self.running.store(true, Ordering::SeqCst);
        *self.state.lock().unwrap() = RecorderState::Recording;
        *self.session.lock().unwrap() = Some(session.clone());

        let session_capture = session.clone();
        let session_flush = session.clone();
        let db_capture = self.db.clone();
        let db_flush = self.db.clone();
        let running_capture = Arc::clone(&self.running);
        let running_flush = Arc::clone(&self.running);
        let power_monitor_arc = Arc::clone(&self.power_monitor);

        {
            let mut pm_guard = self.power_monitor.lock().unwrap();
            let mut pm = PowerMonitor::new();
            let session_pm = session.clone();
            pm.start(Box::new(move |event: PowerEvent| {
                match event {
                    PowerEvent::Suspend | PowerEvent::DisplayOff => {
                        let reason = match event {
                            PowerEvent::Suspend => "system_suspend",
                            PowerEvent::DisplayOff => "display_off",
                            _ => unreachable!(),
                        };
                        session_pm.with(|s| s.auto_pause(reason));
                    }
                    PowerEvent::Resume | PowerEvent::DisplayOn => {
                        session_pm.with(|s| s.auto_resume());
                    }
                }
            }));
            *pm_guard = Some(pm);
        }

        let diff_engine = Arc::new(Mutex::new(DiffEngine::default()));
        let capture_handle = thread::spawn(move || {
            let interval = Duration::from_millis(100);
            loop {
                if !running_capture.load(Ordering::SeqCst) {
                    break;
                }

                if session_capture.is_paused() {
                    thread::sleep(Duration::from_millis(50));
                    continue;
                }

                {
                    let pm_guard = power_monitor_arc.lock().unwrap();
                    if let Some(ref pm) = *pm_guard {
                        if pm.should_pause() {
                            if let Some(reason) = pm.auto_pause_reason() {
                                session_capture.with(|s| s.auto_pause(&reason));
                            }
                            thread::sleep(Duration::from_millis(50));
                            continue;
                        }
                    }
                }

                let ts = session_capture.elapsed_ms();

                let frame = match capturer.capture_frame() {
                    Ok(f) => f,
                    Err(e) => {
                        eprintln!("Capture error: {}", e);
                        thread::sleep(interval);
                        continue;
                    }
                };

                if ScreenCapturer::is_blank_frame(&frame) {
                    session_capture.with(|s| s.auto_pause("blank_frame"));
                    thread::sleep(Duration::from_millis(200));
                    continue;
                }

                let prev_opt = session_capture.read(|s| s.previous_frame.clone());

                if let Some(prev) = prev_opt {
                    let diff_result = {
                        let mut engine = diff_engine.lock().unwrap();
                        engine.compute_diff(&prev, &frame, ts)
                    };
                    match diff_result {
                        Ok(mut frame_diff) => {
                            let sf = session_capture.read(|s| s.scale_factor);
                            for rect in &mut frame_diff.rects {
                                rect.fill_logical(sf);
                            }
                            session_capture.with(|s| {
                                s.pending_frames.push(frame_diff);
                                s.previous_frame = Some(frame);
                            });
                        }
                        Err(e) => eprintln!("Diff computation error: {}", e),
                    }
                } else {
                    session_capture.with(|s| {
                        s.previous_frame = Some(frame);
                    });
                }

                session_capture.with(|s| {
                    s.frame_count = s.frame_count.saturating_add(1);
                });

                thread::sleep(interval);
            }
            let _ = db_capture;
        });

        let flush_handle = thread::spawn(move || {
            loop {
                if !running_flush.load(Ordering::SeqCst) {
                    break;
                }
                thread::sleep(BATCH_FLUSH_INTERVAL);
                let needs_flush = session_flush.read(|s| {
                    s.pending_frames.len() >= MAX_PENDING_FRAMES
                        || !s.pending_frames.is_empty()
                });
                if needs_flush {
                    if let Err(e) = session_flush.with(|s| s.flush_pending(&db_flush)) {
                        eprintln!("Flush error: {}", e);
                    }
                }
            }
        });

        *self.capture_thread.lock().unwrap() = Some(capture_handle);
        *self.flush_thread.lock().unwrap() = Some(flush_handle);

        Ok(recording)
    }

    pub fn pause(&self) -> Result<()> {
        let mut state = self.state.lock().unwrap();
        if *state != RecorderState::Recording {
            anyhow::bail!("Cannot pause: recorder is not recording");
        }
        if let Some(ref session) = *self.session.lock().unwrap() {
            session.with(|s| s.pause());
        }
        *state = RecorderState::Paused;
        Ok(())
    }

    pub fn resume(&self) -> Result<()> {
        let mut state = self.state.lock().unwrap();
        if *state != RecorderState::Paused {
            anyhow::bail!("Cannot resume: recorder is not paused");
        }
        if let Some(ref session) = *self.session.lock().unwrap() {
            session.with(|s| {
                s.auto_paused = false;
                s.auto_paused_reason = None;
                s.resume();
            });
        }
        {
            let mut diff_engine_reset = false;
            if let Some(ref session) = *self.session.lock().unwrap() {
                let was_auto = session.read(|s| s.auto_paused);
                if was_auto {
                    diff_engine_reset = true;
                }
            }
            let _ = diff_engine_reset;
        }
        *state = RecorderState::Recording;
        Ok(())
    }

    pub fn stop(&self) -> Result<Recording> {
        {
            let state = *self.state.lock().unwrap();
            if state == RecorderState::Idle {
                anyhow::bail!("Cannot stop: recorder is idle");
            }
        }

        self.running.store(false, Ordering::SeqCst);

        if let Some(handle) = self.capture_thread.lock().unwrap().take() {
            let _ = handle.join();
        }
        if let Some(handle) = self.flush_thread.lock().unwrap().take() {
            let _ = handle.join();
        }

        {
            let mut pm_guard = self.power_monitor.lock().unwrap();
            if let Some(mut pm) = pm_guard.take() {
                pm.stop();
            }
        }

        let session_opt = self.session.lock().unwrap().take();
        let recording = if let Some(session) = session_opt {
            session
                .with(|s| s.flush_pending(&self.db))
                .ok();

            let (id, title, start_time, width, height, logical_width, logical_height, scale_factor, duration_ms, frame_count) =
                session.read(|s| {
                    (
                        s.id.clone(),
                        s.title.clone(),
                        s.start_time,
                        s.width,
                        s.height,
                        s.logical_width,
                        s.logical_height,
                        s.scale_factor,
                        {
                            let base = s.elapsed_before_pause;
                            if s.is_paused() {
                                base
                            } else {
                                base + s.start_instant.elapsed().as_millis() as u64
                            }
                        },
                        s.frame_count,
                    )
                });

            let end_time = Utc::now().timestamp();

            self.db
                .update_recording_end(&id, end_time, duration_ms, frame_count)
                .context("Failed to update recording end in DB")?;

            let thumbnail = self
                .db
                .get_recording(&id)
                .ok()
                .flatten()
                .and_then(|r| r.thumbnail);

            Recording {
                id,
                title,
                start_time,
                end_time: Some(end_time),
                duration_ms,
                width,
                height,
                logical_width,
                logical_height,
                scale_factor,
                frame_count,
                thumbnail,
                total_size: None,
            }
        } else {
            anyhow::bail!("No active recording session");
        };

        *self.state.lock().unwrap() = RecorderState::Idle;

        Ok(recording)
    }
}
