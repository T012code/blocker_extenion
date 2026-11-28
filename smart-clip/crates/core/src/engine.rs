//! The sync engine: keeps the signed-in session fresh, holds the account's clips, sends
//! new copies to the server and listens for clips from the account's other PCs.
//!
//! Power use: while signed in, unpaused and the PC is unlocked, there is one open
//! WebSocket with a heartbeat every 30 seconds, and a token refresh about once an hour.
//! While paused, locked, asleep or signed out, nothing runs at all.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use chrono::Utc;
use reqwest::Client;
use serde::Serialize;
use tokio::sync::{Notify, mpsc, watch};

use crate::api::{Api, MAX_CLIPS};
use crate::auth::{self, Session};
use crate::content::{self, Skip};
use crate::error::Error;
use crate::image as img;
use crate::model::{Captured, Clip, ClipKind, ClipSummary, Config, NewClip, StoredSession};
use crate::realtime::{self, RtEvent};

/// The server keeps at most this many image clips per account; so does the app.
const MAX_IMAGES: usize = 30;
/// Copies made while offline wait here (oldest dropped first) and go up on reconnect.
const MAX_PENDING_TEXT: usize = 20;
/// Downloaded images kept in memory for quick repeat pastes.
const IMAGE_CACHE: usize = 4;
/// Waits between reconnect attempts, in seconds.
const BACKOFF: [u64; 6] = [2, 5, 10, 30, 60, 120];

/// What the app's windows are told.
pub trait Host: Send + Sync + 'static {
    fn clips_changed(&self, clips: Vec<ClipSummary>);
    fn status_changed(&self, status: Status);
    /// Save (Some) or forget (None) the session on disk.
    fn save_session(&self, session: Option<StoredSession>);
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Status {
    /// The app was built with a server to talk to.
    pub configured: bool,
    pub signed_in: bool,
    pub email: Option<String>,
    /// Listening for clips from the other PCs right now.
    pub online: bool,
    pub paused: bool,
    /// A problem worth showing (offline, signed out...).
    pub message: Option<String>,
}

/// A clip's content, for pasting.
#[derive(Debug, Clone)]
pub enum Content {
    Text(String),
    /// The synced file (WebP or JPEG), as downloaded.
    Image(Arc<Vec<u8>>),
}

struct State {
    session: Option<Session>,
    device_id: String,
    device_name: String,
    clips: Vec<Clip>,
    paused: bool,
    suspended: bool,
    online: bool,
    sync_images: bool,
    pending_text: VecDeque<String>,
    pending_image: Option<Captured>,
    images: VecDeque<(String, Arc<Vec<u8>>)>,
    message: Option<String>,
}

impl State {
    fn ready(&self) -> bool {
        self.session.is_some() && !self.paused && !self.suspended
    }
}

struct Inner {
    api: Api,
    http: Client,
    cfg: Config,
    host: Arc<dyn Host>,
    state: Mutex<State>,
    /// Wakes the sync loop when it may (re)connect.
    wake: Notify,
    /// Tells the live connection to close (paused, locked, signed out).
    stop: watch::Sender<bool>,
    /// The current access token, passed to the live connection when it changes.
    token: watch::Sender<String>,
    /// One refresh at a time: refresh tokens are single-use.
    refreshing: tokio::sync::Mutex<()>,
    /// One upload at a time, in the order things were copied.
    uploading: tokio::sync::Mutex<()>,
}

#[derive(Clone)]
pub struct Engine {
    inner: Arc<Inner>,
}

impl Engine {
    pub fn new(cfg: Config, host: Arc<dyn Host>, device_id: String, device_name: String) -> Engine {
        let http = Client::builder()
            .connect_timeout(Duration::from_secs(15))
            .timeout(Duration::from_secs(60))
            .pool_idle_timeout(Duration::from_secs(90))
            .user_agent(concat!("SmartClip/", env!("CARGO_PKG_VERSION")))
            .build()
            .unwrap_or_default();
        let api = Api::new(http.clone(), cfg.clone());
        Engine {
            inner: Arc::new(Inner {
                api,
                http,
                cfg,
                host,
                state: Mutex::new(State {
                    session: None,
                    device_id,
                    device_name,
                    clips: Vec::new(),
                    paused: false,
                    suspended: false,
                    online: false,
                    sync_images: true,
                    pending_text: VecDeque::new(),
                    pending_image: None,
                    images: VecDeque::new(),
                    message: None,
                }),
                wake: Notify::new(),
                stop: watch::Sender::new(true),
                token: watch::Sender::new(String::new()),
                refreshing: tokio::sync::Mutex::new(()),
                uploading: tokio::sync::Mutex::new(()),
            }),
        }
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.inner.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    // ------------------------------------------------------------ status

    pub fn status(&self) -> Status {
        let s = self.state();
        Status {
            configured: self.inner.cfg.is_set(),
            signed_in: s.session.is_some(),
            email: s.session.as_ref().map(|x| x.email.clone()),
            online: s.online,
            paused: s.paused,
            message: s.message.clone(),
        }
    }

    fn emit_status(&self) {
        self.inner.host.status_changed(self.status());
    }

    /// The clips for the lists, newest first, without expired ones.
    pub fn clips(&self) -> Vec<ClipSummary> {
        let now = Utc::now();
        let s = self.state();
        s.clips.iter().filter(|c| !c.is_expired(now)).map(|c| c.summary(&s.device_id)).collect()
    }

    fn emit_clips(&self) {
        self.inner.host.clips_changed(self.clips());
    }

    /// Applies a change that may let the sync loop connect, or must make it stop.
    fn changed_readiness(&self, s: &State) {
        if s.ready() {
            self.inner.wake.notify_one();
        } else {
            self.inner.stop.send_replace(true);
        }
    }

    // ------------------------------------------------------------ settings

    pub fn set_paused(&self, paused: bool) {
        {
            let mut s = self.state();
            s.paused = paused;
            if paused {
                s.online = false;
            }
            self.changed_readiness(&s);
        }
        self.emit_status();
    }

    /// The PC was locked or went to sleep (true), or is back (false).
    pub fn set_suspended(&self, suspended: bool) {
        let mut s = self.state();
        if s.suspended == suspended {
            return;
        }
        s.suspended = suspended;
        if suspended {
            s.online = false;
        }
        self.changed_readiness(&s);
    }

    pub fn set_device_name(&self, name: &str) {
        let name: String = name.trim().chars().take(64).collect();
        if !name.is_empty() {
            self.state().device_name = name;
        }
    }

    pub fn set_sync_images(&self, on: bool) {
        let mut s = self.state();
        s.sync_images = on;
        if !on {
            s.pending_image = None;
        }
    }

    // ------------------------------------------------------------ signing in

    /// Picks up the session saved last time.
    pub fn restore(&self, stored: StoredSession) {
        let mut s = self.state();
        self.inner.token.send_replace(String::new());
        s.session = Some(Session::restored(&stored));
        self.changed_readiness(&s);
    }

    pub async fn send_code(&self, email: &str) -> Result<(), Error> {
        if !self.inner.cfg.is_set() {
            return Err(Error::NotConfigured);
        }
        auth::send_code(&self.inner.http, &self.inner.cfg, email).await
    }

    pub async fn verify_code(&self, email: &str, code: &str) -> Result<(), Error> {
        if !self.inner.cfg.is_set() {
            return Err(Error::NotConfigured);
        }
        let session = auth::verify_code(&self.inner.http, &self.inner.cfg, email, code).await?;
        self.inner.host.save_session(Some(session.stored()));
        self.inner.token.send_replace(session.access_token.clone());
        {
            let mut s = self.state();
            s.session = Some(session);
            s.clips.clear();
            s.images.clear();
            s.message = None;
            self.changed_readiness(&s);
        }
        self.emit_status();
        self.emit_clips();
        Ok(())
    }

    /// Signs out here (the other PCs stay signed in).
    pub async fn sign_out(&self) {
        let session = self.forget(None);
        if let Some(session) = session {
            auth::sign_out(&self.inner.http, &self.inner.cfg, &session).await;
        }
    }

    /// Drops the session and everything synced, locally.
    fn forget(&self, message: Option<String>) -> Option<Session> {
        let session = {
            let mut s = self.state();
            let session = s.session.take();
            s.clips.clear();
            s.images.clear();
            s.pending_text.clear();
            s.pending_image = None;
            s.online = false;
            s.message = message;
            self.changed_readiness(&s);
            session
        };
        self.inner.token.send_replace(String::new());
        self.inner.host.save_session(None);
        self.emit_status();
        self.emit_clips();
        session
    }

    /// A fresh access token, refreshing it first if it's about to run out.
    async fn token(&self) -> Result<(String, String), Error> {
        let current = |e: &Engine| -> Result<Option<(String, String)>, Error> {
            let s = e.state();
            let session = s.session.as_ref().ok_or(Error::SignedOut)?;
            Ok((session.refresh_in() > 0 && !session.access_token.is_empty())
                .then(|| (session.access_token.clone(), session.user_id.clone())))
        };
        if let Some(t) = current(self)? {
            return Ok(t);
        }
        let _one_at_a_time = self.inner.refreshing.lock().await;
        // Another task may have refreshed while this one waited.
        if let Some(t) = current(self)? {
            return Ok(t);
        }
        let old = self.state().session.clone().ok_or(Error::SignedOut)?;
        match auth::refresh(&self.inner.http, &self.inner.cfg, &old).await {
            Ok(fresh) => {
                let out = (fresh.access_token.clone(), fresh.user_id.clone());
                {
                    let mut s = self.state();
                    // Signed out (or in as someone else) meanwhile: drop this refresh.
                    if s.session.as_ref().map(|x| &x.refresh_token) != Some(&old.refresh_token) {
                        return Err(Error::SignedOut);
                    }
                    s.session = Some(fresh.clone());
                }
                self.inner.host.save_session(Some(fresh.stored()));
                self.inner.token.send_replace(fresh.access_token);
                Ok(out)
            }
            Err(Error::SignedOut) => {
                self.forget(Some(Error::SignedOut.to_string()));
                Err(Error::SignedOut)
            }
            Err(e) => Err(e),
        }
    }

    // ------------------------------------------------------------ the sync loop

    /// Runs for the life of the app.
    pub async fn run(self) {
        let mut failures = 0usize;
        loop {
            // Wait until signed in, not paused and not locked.
            loop {
                let woken = self.inner.wake.notified();
                {
                    let s = self.state();
                    if s.ready() {
                        self.inner.stop.send_replace(false);
                        break;
                    }
                }
                woken.await;
            }
            match self.connected().await {
                Ok(()) => failures = 0,
                Err(e) => {
                    {
                        let mut s = self.state();
                        s.online = false;
                        if s.session.is_some() && failures >= 1 {
                            s.message = Some(e.to_string());
                        }
                    }
                    self.emit_status();
                    if matches!(e, Error::SignedOut) {
                        // The token may just have run out: refresh and try again at once.
                        // A second refusal in a row waits like any other failure.
                        if let Some(session) = self.state().session.as_mut() {
                            session.expires_at = 0;
                        }
                    }
                    let wait = BACKOFF[failures.min(BACKOFF.len() - 1)];
                    failures += 1;
                    if !(matches!(e, Error::SignedOut) && failures == 1) {
                        // Waits, unless something changes (unlock, resume, sign-in...).
                        let _ = tokio::time::timeout(Duration::from_secs(wait), self.inner.wake.notified()).await;
                    }
                }
            }
            {
                let mut s = self.state();
                s.online = false;
            }
            self.emit_status();
        }
    }

    /// One connection: listens until it drops or is told to stop.
    async fn connected(&self) -> Result<(), Error> {
        let (_, user_id) = self.token().await?;
        let (tx, mut rx) = mpsc::unbounded_channel();
        let url = self.inner.cfg.realtime_url();
        let live = realtime::run(&url, &user_id, self.inner.token.subscribe(), tx, self.inner.stop.subscribe());
        tokio::pin!(live);
        loop {
            let refresh_in = self.state().session.as_ref().map(Session::refresh_in).unwrap_or(0);
            tokio::select! {
                r = &mut live => return r,
                Some(event) = rx.recv() => match event {
                    RtEvent::Joined => {
                        self.catch_up().await?;
                        {
                            let mut s = self.state();
                            s.online = true;
                            s.message = None;
                        }
                        self.emit_status();
                        self.flush_pending().await;
                    }
                    RtEvent::Upsert(clip) => self.apply_upsert(*clip),
                    RtEvent::Delete(id) => self.apply_delete(&id),
                },
                _ = tokio::time::sleep(Duration::from_secs(refresh_in.max(5))) => {
                    self.token().await?;
                }
            }
        }
    }

    /// Brings the list up to date: fetches the ids of all clips, then only the clips this
    /// PC doesn't have yet (or that were bumped). Clips deleted meanwhile drop out.
    async fn catch_up(&self) -> Result<(), Error> {
        let (token, _) = self.token().await?;
        let stamps = self.inner.api.stamps(&token).await?;
        let need: Vec<String> = {
            let s = self.state();
            let known: HashMap<&str, _> = s.clips.iter().map(|c| (c.id.as_str(), c.created_at)).collect();
            stamps
                .iter()
                .filter(|st| known.get(st.id.as_str()) != Some(&st.created_at))
                .map(|st| st.id.clone())
                .collect()
        };
        let mut fresh: HashMap<String, Clip> = HashMap::new();
        for chunk in need.chunks(40) {
            for clip in self.inner.api.clips_by_id(&token, chunk).await? {
                fresh.insert(clip.id.clone(), clip);
            }
        }
        {
            let now = Utc::now();
            let mut s = self.state();
            let mut old: HashMap<String, Clip> = s.clips.drain(..).map(|c| (c.id.clone(), c)).collect();
            s.clips = stamps
                .iter()
                .filter_map(|st| fresh.remove(&st.id).or_else(|| old.remove(&st.id)))
                .filter(|c| !c.is_expired(now))
                .collect();
            s.clips.sort_by_key(|c| std::cmp::Reverse(c.created_at));
        }
        self.emit_clips();
        Ok(())
    }

    fn apply_upsert(&self, clip: Clip) {
        if clip.is_expired(Utc::now()) {
            return;
        }
        {
            let mut s = self.state();
            // An upload that finished after signing out doesn't bring its clip back.
            if s.session.is_none() {
                return;
            }
            s.clips.retain(|c| c.id != clip.id && c.hash != clip.hash);
            let at = s.clips.partition_point(|c| c.created_at > clip.created_at);
            s.clips.insert(at, clip);
            s.clips.truncate(MAX_CLIPS);
            // The same cap as the server: at most 30 images, the oldest go.
            let mut images = 0;
            s.clips.retain(|c| {
                if c.kind == ClipKind::Image {
                    images += 1;
                    images <= MAX_IMAGES
                } else {
                    true
                }
            });
        }
        self.emit_clips();
    }

    fn apply_delete(&self, id: &str) {
        let removed = {
            let mut s = self.state();
            let before = s.clips.len();
            s.clips.retain(|c| c.id != id);
            before != s.clips.len()
        };
        if removed {
            self.emit_clips();
        }
    }

    // ------------------------------------------------------------ copying

    /// Something was just copied on this PC. Sends it to the account's other PCs.
    pub async fn capture(&self, captured: Captured) {
        {
            let s = self.state();
            if s.session.is_none() || s.paused {
                return;
            }
            if matches!(captured, Captured::Image { .. }) && !s.sync_images {
                return;
            }
        }
        let _in_order = self.inner.uploading.lock().await;
        match self.upload(&captured).await {
            Ok(()) => {}
            Err(e) if e.is_offline() => {
                let mut s = self.state();
                match captured {
                    Captured::Text(t) => {
                        s.pending_text.retain(|p| *p != t);
                        s.pending_text.push_back(t);
                        while s.pending_text.len() > MAX_PENDING_TEXT {
                            s.pending_text.pop_front();
                        }
                    }
                    image => s.pending_image = Some(image),
                }
            }
            Err(Error::SignedOut) => {}
            Err(e) => {
                tracing::warn!("clip not synced: {e}");
            }
        }
    }

    /// Sends what was copied while offline.
    async fn flush_pending(&self) {
        let _in_order = self.inner.uploading.lock().await;
        loop {
            let next = {
                let mut s = self.state();
                s.pending_text.pop_front().map(Captured::Text).or_else(|| s.pending_image.take())
            };
            let Some(item) = next else { break };
            if let Err(e) = self.upload(&item).await {
                if e.is_offline() {
                    // Still offline: put it back and try on the next reconnect.
                    let mut s = self.state();
                    match item {
                        Captured::Text(t) => s.pending_text.push_front(t),
                        image => s.pending_image = Some(image),
                    }
                }
                break;
            }
        }
    }

    async fn upload(&self, captured: &Captured) -> Result<(), Error> {
        let (device_id, device_name) = {
            let s = self.state();
            (s.device_id.clone(), s.device_name.clone())
        };
        let new = match captured {
            Captured::Text(text) => {
                match content::check_text(text) {
                    Ok(()) => {}
                    Err(Skip::Empty) | Err(Skip::TooBig) => return Ok(()),
                }
                let hash = content::text_hash(text);
                // Copying what's already the newest clip changes nothing.
                if self.state().clips.first().is_some_and(|c| c.hash == hash) {
                    return Ok(());
                }
                NewClip {
                    device_id,
                    device_name,
                    kind: ClipKind::Text,
                    hash,
                    text: Some(text.clone()),
                    thumb: None,
                    image_path: None,
                    width: None,
                    height: None,
                    bytes: text.len() as i64,
                }
            }
            Captured::Image { rgba, width, height } => {
                let hash = content::image_hash(rgba, *width, *height);
                let existing = {
                    let s = self.state();
                    if s.clips.first().is_some_and(|c| c.hash == hash) {
                        return Ok(());
                    }
                    s.clips.iter().find(|c| c.hash == hash).cloned()
                };
                match existing {
                    // The same picture again: bump it without uploading the file again.
                    Some(c) => NewClip {
                        device_id,
                        device_name,
                        kind: ClipKind::Image,
                        hash,
                        text: None,
                        thumb: c.thumb,
                        image_path: c.image_path,
                        width: c.width,
                        height: c.height,
                        bytes: c.bytes,
                    },
                    None => {
                        let (rgba, w, h) = (rgba.clone(), *width, *height);
                        let encoded =
                            tokio::task::spawn_blocking(move || img::encode_for_sync(&rgba, w, h)).await.ok().flatten();
                        // A picture too big to fit even when shrunk stays on this PC.
                        let Some(encoded) = encoded else { return Ok(()) };
                        let (token, user_id) = self.token().await?;
                        let path = format!("{user_id}/{}.{}", uuid::Uuid::new_v4(), encoded.ext);
                        let bytes = encoded.bytes.len() as i64;
                        self.inner.api.upload_image(&token, &path, encoded.bytes.clone(), encoded.mime).await?;
                        self.cache_image(&path, Arc::new(encoded.bytes));
                        NewClip {
                            device_id,
                            device_name,
                            kind: ClipKind::Image,
                            hash,
                            text: None,
                            thumb: Some(encoded.thumb_b64),
                            image_path: Some(path),
                            width: Some(encoded.width as i32),
                            height: Some(encoded.height as i32),
                            bytes,
                        }
                    }
                }
            }
        };
        let (token, _) = self.token().await?;
        let saved = self.inner.api.upsert(&token, &new).await?;
        self.apply_upsert(saved);
        Ok(())
    }

    fn cache_image(&self, path: &str, bytes: Arc<Vec<u8>>) {
        let mut s = self.state();
        s.images.retain(|(p, _)| p != path);
        s.images.push_back((path.to_owned(), bytes));
        while s.images.len() > IMAGE_CACHE {
            s.images.pop_front();
        }
    }

    // ------------------------------------------------------------ pasting and tidying

    /// A clip's full content, downloading the image file if needed.
    pub async fn content(&self, id: &str) -> Result<Content, Error> {
        let clip = self
            .state()
            .clips
            .iter()
            .find(|c| c.id == id)
            .cloned()
            .ok_or_else(|| Error::Invalid("That clip has expired or was deleted.".into()))?;
        match clip.kind {
            ClipKind::Text => Ok(Content::Text(clip.text.unwrap_or_default())),
            ClipKind::Image => {
                let path = clip.image_path.unwrap_or_default();
                if let Some((_, bytes)) = self.state().images.iter().find(|(p, _)| *p == path) {
                    return Ok(Content::Image(bytes.clone()));
                }
                let (token, _) = self.token().await?;
                let bytes = Arc::new(self.inner.api.download_image(&token, &path).await?);
                self.cache_image(&path, bytes.clone());
                Ok(Content::Image(bytes))
            }
        }
    }

    /// Deletes a clip on every PC.
    pub async fn delete(&self, id: &str) -> Result<(), Error> {
        let clip = {
            let mut s = self.state();
            let clip = s.clips.iter().find(|c| c.id == id).cloned();
            s.clips.retain(|c| c.id != id);
            clip
        };
        self.emit_clips();
        let Some(clip) = clip else { return Ok(()) };
        let (token, _) = self.token().await?;
        self.inner.api.delete(&token, id).await?;
        if let Some(path) = clip.image_path {
            // Best effort: the hourly clean-up removes any file left behind.
            let _ = self.inner.api.remove_images(&token, &[path]).await;
        }
        Ok(())
    }

    /// Deletes every clip of the account, on every PC.
    pub async fn clear_all(&self) -> Result<(), Error> {
        let paths: Vec<String> = {
            let mut s = self.state();
            let paths = s.clips.iter().filter_map(|c| c.image_path.clone()).collect();
            s.clips.clear();
            s.images.clear();
            s.pending_text.clear();
            s.pending_image = None;
            paths
        };
        self.emit_clips();
        let (token, _) = self.token().await?;
        self.inner.api.delete_all(&token).await?;
        let _ = self.inner.api.remove_images(&token, &paths).await;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration as Days;

    #[derive(Default)]
    struct Recorder {
        clips: Mutex<Vec<Vec<ClipSummary>>>,
        saved: Mutex<Vec<Option<StoredSession>>>,
    }
    impl Host for Recorder {
        fn clips_changed(&self, clips: Vec<ClipSummary>) {
            self.clips.lock().unwrap().push(clips);
        }
        fn status_changed(&self, _: Status) {}
        fn save_session(&self, s: Option<StoredSession>) {
            self.saved.lock().unwrap().push(s);
        }
    }

    fn clip(id: &str, hash: &str, kind: ClipKind, minutes_ago: i64) -> Clip {
        let created = Utc::now() - Days::minutes(minutes_ago);
        Clip {
            id: id.into(),
            device_id: "other".into(),
            device_name: "Laptop".into(),
            kind,
            hash: hash.into(),
            text: (kind == ClipKind::Text).then(|| format!("text {id}")),
            thumb: (kind == ClipKind::Image).then(|| "AAAA".into()),
            image_path: (kind == ClipKind::Image).then(|| format!("u/{id}.webp")),
            width: None,
            height: None,
            bytes: 1,
            created_at: created,
            expires_at: created + Days::days(1),
        }
    }

    fn engine() -> (Engine, Arc<Recorder>) {
        let rec = Arc::new(Recorder::default());
        let e = Engine::new(Config::default(), rec.clone(), "this".into(), "Desk".into());
        e.restore(StoredSession { refresh_token: "r".into(), user_id: "u".into(), email: "a@b.co".into() });
        (e, rec)
    }

    #[test]
    fn upserts_keep_newest_first_and_replace_by_id_or_hash() {
        let (e, rec) = engine();
        e.apply_upsert(clip("a", "h1", ClipKind::Text, 10));
        e.apply_upsert(clip("b", "h2", ClipKind::Text, 5));
        e.apply_upsert(clip("c", "h3", ClipKind::Text, 20));
        let ids: Vec<String> = e.clips().into_iter().map(|c| c.id).collect();
        assert_eq!(ids, ["b", "a", "c"]);
        // The same content copied again (same hash) replaces the old clip and goes on top.
        e.apply_upsert(clip("a", "h1", ClipKind::Text, 0));
        let ids: Vec<String> = e.clips().into_iter().map(|c| c.id).collect();
        assert_eq!(ids, ["a", "b", "c"]);
        e.apply_delete("b");
        assert_eq!(e.clips().len(), 2);
        assert!(!rec.clips.lock().unwrap().is_empty());
        // Signed out: late arrivals are ignored.
        e.forget(None);
        e.apply_upsert(clip("z", "hz", ClipKind::Text, 0));
        assert!(e.clips().is_empty());
    }

    #[test]
    fn expired_clips_are_dropped() {
        let (e, _) = engine();
        let mut old = clip("x", "h", ClipKind::Text, 60);
        old.expires_at = Utc::now() - Days::seconds(1);
        e.apply_upsert(old.clone());
        assert!(e.clips().is_empty());
        e.state().clips.push(old);
        assert!(e.clips().is_empty(), "filtered when listed too");
    }

    #[test]
    fn caps_images_and_total() {
        let (e, _) = engine();
        for i in 0..40 {
            e.apply_upsert(clip(&format!("i{i}"), &format!("hi{i}"), ClipKind::Image, 100 - i));
        }
        let list = e.clips();
        assert_eq!(list.len(), MAX_IMAGES);
        assert_eq!(list[0].id, "i39");
        for i in 0..250 {
            e.apply_upsert(clip(&format!("t{i}"), &format!("ht{i}"), ClipKind::Text, 1000 - i));
        }
        assert_eq!(e.clips().len(), MAX_CLIPS);
    }

    #[test]
    fn summaries_mark_this_device_and_thumbs() {
        let (e, _) = engine();
        let mut mine = clip("m", "h", ClipKind::Image, 1);
        mine.device_id = "this".into();
        e.apply_upsert(mine);
        let s = &e.clips()[0];
        assert!(s.this_device);
        assert_eq!(s.thumb.as_deref(), Some("data:image/jpeg;base64,AAAA"));
    }

    #[tokio::test]
    async fn without_a_server_nothing_is_sent() {
        let rec = Arc::new(Recorder::default());
        let e = Engine::new(Config::default(), rec.clone(), "this".into(), "Desk".into());
        assert!(!e.status().configured);
        assert_eq!(e.send_code("a@b.co").await, Err(Error::NotConfigured));
        // Not signed in: copies are ignored.
        e.capture(Captured::Text("hello".into())).await;
        assert!(e.clips().is_empty());
        e.restore(StoredSession { refresh_token: "r".into(), user_id: "u".into(), email: "a@b.co".into() });
        assert!(e.status().signed_in);
        e.forget(None);
        assert!(!e.status().signed_in);
        assert_eq!(rec.saved.lock().unwrap().last(), Some(&None));
    }

    #[tokio::test]
    async fn offline_copies_wait_for_the_connection() {
        let rec = Arc::new(Recorder::default());
        // A server address nothing listens on: every request fails as offline.
        let cfg = Config { url: "https://127.0.0.1:9".into(), publishable_key: "k".into() };
        let e = Engine::new(cfg, rec, "this".into(), "Desk".into());
        e.restore(StoredSession { refresh_token: "r".into(), user_id: "u".into(), email: "a@b.co".into() });
        e.capture(Captured::Text("one".into())).await;
        e.capture(Captured::Text("two".into())).await;
        e.capture(Captured::Text("one".into())).await;
        let s = e.state();
        assert_eq!(s.pending_text.iter().map(String::as_str).collect::<Vec<_>>(), ["two", "one"]);
        assert!(s.session.is_some(), "being offline doesn't sign you out");
    }
}
