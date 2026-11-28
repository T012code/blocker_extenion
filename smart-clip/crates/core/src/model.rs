//! What a clip is, on the server and in the app.

use base64::Engine as _;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// The Supabase project the app talks to: its URL and its publishable (public) key.
/// Both are safe to ship inside the app; row-level security protects the data.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Config {
    pub url: String,
    pub publishable_key: String,
}

impl Config {
    pub fn is_set(&self) -> bool {
        self.url.starts_with("https://") && !self.publishable_key.trim().is_empty()
    }

    pub fn base(&self) -> &str {
        self.url.trim_end_matches('/')
    }

    /// The Realtime WebSocket address.
    pub fn realtime_url(&self) -> String {
        let ws = self.base().replacen("https://", "wss://", 1);
        format!("{ws}/realtime/v1/websocket?apikey={}&vsn=1.0.0", self.publishable_key.trim())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ClipKind {
    Text,
    Image,
}

/// A clip as stored in the `clips` table.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Clip {
    pub id: String,
    pub device_id: String,
    pub device_name: String,
    pub kind: ClipKind,
    pub hash: String,
    #[serde(default)]
    pub text: Option<String>,
    /// Base64 JPEG thumbnail, for images.
    #[serde(default)]
    pub thumb: Option<String>,
    /// The image file in the "clips" bucket: `<user id>/<uuid>.webp` or `.jpg`.
    #[serde(default)]
    pub image_path: Option<String>,
    #[serde(default)]
    pub width: Option<i32>,
    #[serde(default)]
    pub height: Option<i32>,
    #[serde(default)]
    pub bytes: i64,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

impl Clip {
    pub fn is_expired(&self, now: DateTime<Utc>) -> bool {
        self.expires_at <= now
    }

    /// The short version shown in lists.
    pub fn summary(&self, this_device: &str) -> ClipSummary {
        let (excerpt, chars) = match &self.text {
            Some(t) => (crate::content::excerpt(t, 240), t.chars().count()),
            None => (String::new(), 0),
        };
        ClipSummary {
            id: self.id.clone(),
            kind: self.kind,
            excerpt,
            chars,
            device_name: self.device_name.clone(),
            this_device: self.device_id == this_device,
            created_at: self.created_at.to_rfc3339(),
            thumb: self.thumb.as_ref().map(|b| format!("data:image/jpeg;base64,{b}")),
            width: self.width,
            height: self.height,
        }
    }
}

/// A new clip (or a repeat copy of an old one) sent to the server. The server fills in
/// the id, the account and the times.
#[derive(Debug, Clone, Serialize)]
pub struct NewClip {
    pub device_id: String,
    pub device_name: String,
    pub kind: ClipKind,
    pub hash: String,
    pub text: Option<String>,
    pub thumb: Option<String>,
    pub image_path: Option<String>,
    pub width: Option<i32>,
    pub height: Option<i32>,
    pub bytes: i64,
}

/// What the app's windows see of a clip.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ClipSummary {
    pub id: String,
    pub kind: ClipKind,
    pub excerpt: String,
    pub chars: usize,
    pub device_name: String,
    pub this_device: bool,
    pub created_at: String,
    pub thumb: Option<String>,
    pub width: Option<i32>,
    pub height: Option<i32>,
}

/// Something just copied on this PC.
#[derive(Debug, Clone)]
pub enum Captured {
    Text(String),
    /// Straight RGBA pixels.
    Image {
        rgba: Vec<u8>,
        width: u32,
        height: u32,
    },
}

/// What's kept on disk between runs (the app protects it with Windows' DPAPI).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StoredSession {
    pub refresh_token: String,
    pub user_id: String,
    pub email: String,
}

pub(crate) fn b64(data: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(data)
}
