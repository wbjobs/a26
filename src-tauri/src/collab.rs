use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum CollabEvent {
    Cursor {
        user_id: String,
        x: f64,
        y: f64,
        timestamp_ms: u64,
    },
    Play {
        user_id: String,
        timestamp_ms: u64,
    },
    Pause {
        user_id: String,
        timestamp_ms: u64,
    },
    Seek {
        user_id: String,
        target_ms: u64,
    },
    Annotation {
        user_id: String,
        timestamp_ms: u64,
        item: crate::AnnotationItem,
    },
    Chat {
        user_id: String,
        text: String,
        timestamp_ms: u64,
    },
    UserJoin {
        user_id: String,
        name: String,
        color: String,
    },
    UserLeave {
        user_id: String,
    },
    HostSync {
        user_id: String,
        is_playing: bool,
        current_ms: u64,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerInfo {
    pub user_id: String,
    pub name: String,
    pub color: String,
    pub cursor_x: Option<f64>,
    pub cursor_y: Option<f64>,
    pub last_seen_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoomInfo {
    pub room_code: String,
    pub host_id: String,
    pub recording_id: String,
    pub peers: Vec<PeerInfo>,
    pub is_playing: bool,
    pub current_ms: u64,
}

type EventCallback = Box<dyn Fn(CollabEvent) + Send + Sync + 'static>;

struct RoomState {
    room_code: String,
    host_id: String,
    recording_id: String,
    peers: HashMap<String, PeerInfo>,
    is_playing: bool,
    current_ms: u64,
    callbacks: Vec<EventCallback>,
    event_buffer: Vec<(Instant, CollabEvent)>,
}

#[derive(Clone)]
pub struct CollabEngine {
    rooms: Arc<Mutex<HashMap<String, Arc<Mutex<RoomState>>>>>,
    local_user_id: String,
    local_name: String,
    local_color: String,
}

fn generate_room_code() -> String {
    const CHARSET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";
    let mut rng = rand_xor();
    (0..6)
        .map(|_| {
            let idx = (rng.next_u32() as usize) % CHARSET.len();
            CHARSET[idx] as char
        })
        .collect()
}

struct RandXorshift(u64);

fn rand_xor() -> RandXorshift {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64;
    RandXorshift(now | 1)
}

impl RandXorshift {
    fn next_u32(&mut self) -> u32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x as u32
    }
}

const USER_COLORS: &[&str] = &[
    "#FF6B6B", "#4ECDC4", "#45B7D1", "#96CEB4", "#FFEAA7",
    "#DDA0DD", "#FF8C42", "#6C5CE7", "#A8E6CF", "#FFD93D",
];

impl CollabEngine {
    pub fn new() -> Self {
        Self {
            rooms: Arc::new(Mutex::new(HashMap::new())),
            local_user_id: Uuid::new_v4().to_string(),
            local_name: String::from("Guest"),
            local_color: String::from(USER_COLORS[0]),
        }
    }

    pub fn local_user_id(&self) -> &str {
        &self.local_user_id
    }

    pub fn set_local_name(&mut self, name: &str) {
        self.local_name = name.to_string();
    }

    pub fn set_local_color(&mut self, color_idx: usize) {
        let idx = color_idx % USER_COLORS.len();
        self.local_color = USER_COLORS[idx].to_string();
    }

    pub fn create_room(&self, recording_id: &str) -> Result<String> {
        let room_code = generate_room_code();
        let host_id = self.local_user_id.clone();
        let color = self.local_color.clone();
        let name = self.local_name.clone();

        let mut peer = PeerInfo {
            user_id: host_id.clone(),
            name,
            color,
            cursor_x: None,
            cursor_y: None,
            last_seen_ms: 0,
        };

        let mut peers = HashMap::new();
        peers.insert(host_id.clone(), peer.clone());

        let room = Arc::new(Mutex::new(RoomState {
            room_code: room_code.clone(),
            host_id: host_id.clone(),
            recording_id: recording_id.to_string(),
            peers,
            is_playing: false,
            current_ms: 0,
            callbacks: Vec::new(),
            event_buffer: Vec::new(),
        }));

        let mut rooms = self.rooms.lock().unwrap();
        rooms.insert(room_code.clone(), room);
        Ok(room_code)
    }

    pub fn join_room(&self, room_code: &str) -> Result<RoomInfo> {
        let rooms = self.rooms.lock().unwrap();
        let room = rooms
            .get(room_code)
            .with_context(|| format!("Room {} not found", room_code))?;

        let mut room_state = room.lock().unwrap();
        let idx = room_state.peers.len() % USER_COLORS.len();
        let color = USER_COLORS[idx].to_string();

        let peer = PeerInfo {
            user_id: self.local_user_id.clone(),
            name: self.local_name.clone(),
            color: color.clone(),
            cursor_x: None,
            cursor_y: None,
            last_seen_ms: 0,
        };

        room_state.peers.insert(self.local_user_id.clone(), peer);

        Self::broadcast_event(&room_state, CollabEvent::UserJoin {
            user_id: self.local_user_id.clone(),
            name: self.local_name.clone(),
            color,
        });

        Ok(RoomInfo {
            room_code: room_state.room_code.clone(),
            host_id: room_state.host_id.clone(),
            recording_id: room_state.recording_id.clone(),
            peers: room_state.peers.values().cloned().collect(),
            is_playing: room_state.is_playing,
            current_ms: room_state.current_ms,
        })
    }

    pub fn leave_room(&self, room_code: &str) -> Result<()> {
        let rooms = self.rooms.lock().unwrap();
        if let Some(room) = rooms.get(room_code) {
            let mut room_state = room.lock().unwrap();
            room_state.peers.remove(&self.local_user_id);
            Self::broadcast_event(&room_state, CollabEvent::UserLeave {
                user_id: self.local_user_id.clone(),
            });
        }
        Ok(())
    }

    pub fn get_room_info(&self, room_code: &str) -> Option<RoomInfo> {
        let rooms = self.rooms.lock().unwrap();
        let room = rooms.get(room_code)?;
        let room_state = room.lock().unwrap();
        Some(RoomInfo {
            room_code: room_state.room_code.clone(),
            host_id: room_state.host_id.clone(),
            recording_id: room_state.recording_id.clone(),
            peers: room_state.peers.values().cloned().collect(),
            is_playing: room_state.is_playing,
            current_ms: room_state.current_ms,
        })
    }

    pub fn send_event(&self, room_code: &str, event: CollabEvent) -> Result<()> {
        let rooms = self.rooms.lock().unwrap();
        let room = rooms
            .get(room_code)
            .with_context(|| format!("Room {} not found", room_code))?;

        let mut room_state = room.lock().unwrap();

        match &event {
            CollabEvent::Cursor { user_id, x, y, .. } => {
                if let Some(p) = room_state.peers.get_mut(user_id) {
                    p.cursor_x = Some(*x);
                    p.cursor_y = Some(*y);
                }
            }
            CollabEvent::Play { .. } => {
                room_state.is_playing = true;
            }
            CollabEvent::Pause { .. } => {
                room_state.is_playing = false;
            }
            CollabEvent::Seek { target_ms, .. } => {
                room_state.current_ms = *target_ms;
                room_state.is_playing = false;
            }
            CollabEvent::HostSync { is_playing, current_ms, .. } => {
                room_state.is_playing = *is_playing;
                room_state.current_ms = *current_ms;
            }
            _ => {}
        }

        Self::broadcast_event(&room_state, event);
        Ok(())
    }

    pub fn on_event(&self, room_code: &str, callback: EventCallback) -> Result<()> {
        let rooms = self.rooms.lock().unwrap();
        let room = rooms
            .get(room_code)
            .with_context(|| format!("Room {} not found", room_code))?;
        let mut room_state = room.lock().unwrap();
        room_state.callbacks.push(callback);
        Ok(())
    }

    fn broadcast_event(room_state: &RoomState, event: CollabEvent) {
        for cb in &room_state.callbacks {
            cb(event.clone());
        }
    }
}

impl Default for CollabEngine {
    fn default() -> Self {
        Self::new()
    }
}
