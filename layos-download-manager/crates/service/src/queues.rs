//! Download queues: named lists that download a few files at a time, in order.
//!
//! A running queue keeps up to `max_active` of its downloads going and starts the next
//! one whenever one finishes. Stopping a queue pauses its downloads. Deleting a queue
//! either keeps its downloads (they leave the queue) or removes them from the list.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use ldm_engine::{DownloadStatus, Engine, Event};
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;

const KEY: &str = "queues";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Queue {
    pub id: i64,
    pub name: String,
    /// Downloads running at once.
    pub max_active: u8,
    pub running: bool,
    /// Start and stop it at set times. `None` = only by hand.
    #[serde(default)]
    pub schedule: Option<Schedule>,
}

/// When a queue runs by itself, and what happens once it has nothing left to do.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schedule {
    /// "HH:MM", local time.
    #[serde(default)]
    pub start: Option<String>,
    #[serde(default)]
    pub stop: Option<String>,
    /// Days it runs, 0 = Monday … 6 = Sunday. Empty = every day.
    #[serde(default)]
    pub days: Vec<u8>,
    #[serde(default)]
    pub when_done: WhenDone,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum WhenDone {
    #[default]
    Nothing,
    /// Close Layó's Download Manager.
    Quit,
    /// Put the computer to sleep.
    Sleep,
    /// Shut the computer down (with a minute to cancel).
    ShutDown,
}

/// Parses "HH:MM" into minutes after midnight.
fn minutes(hhmm: &str) -> Option<u32> {
    let (h, m) = hhmm.trim().split_once(':')?;
    let (h, m): (u32, u32) = (h.parse().ok()?, m.parse().ok()?);
    (h < 24 && m < 60).then_some(h * 60 + m)
}

impl Schedule {
    pub fn validate(&self) -> Result<(), String> {
        for t in [&self.start, &self.stop].into_iter().flatten() {
            minutes(t).ok_or_else(|| format!("\"{t}\" isn't a time like 22:30"))?;
        }
        if self.days.iter().any(|d| *d > 6) {
            return Err("days are 0 (Monday) to 6 (Sunday)".into());
        }
        Ok(())
    }
}

pub struct Queues {
    engine: Engine,
    list: Mutex<Vec<Queue>>,
    changed: broadcast::Sender<Vec<Queue>>,
    /// A queue finished everything it had; the app carries out `when_done`.
    finished: broadcast::Sender<(Queue, WhenDone)>,
    /// The minute each schedule last acted, so it acts once per minute.
    fired: Mutex<std::collections::HashMap<(i64, bool), i64>>,
    /// Running queues that have had work since they started.
    had_work: Mutex<std::collections::HashSet<i64>>,
}

impl Queues {
    /// Loads the saved queues and starts keeping running queues busy.
    pub fn start(engine: Engine) -> Arc<Self> {
        let list: Vec<Queue> = engine
            .load_setting(KEY)
            .ok()
            .flatten()
            .and_then(|j| serde_json::from_str(&j).ok())
            .unwrap_or_default();
        let (changed, _) = broadcast::channel(64);
        let (finished, _) = broadcast::channel(16);
        let me = Arc::new(Self {
            engine,
            list: Mutex::new(list),
            changed,
            finished,
            fired: Mutex::default(),
            had_work: Mutex::default(),
        });
        let weak = Arc::downgrade(&me);
        let mut events = me.engine.subscribe();
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(Duration::from_millis(500));
            loop {
                tokio::select! {
                    ev = events.recv() => match ev {
                        // Something finished or stopped: maybe start the next one.
                        Ok(Event::Updated { .. } | Event::Removed { .. }) | Err(broadcast::error::RecvError::Lagged(_)) => {}
                        Ok(_) => continue,
                        Err(_) => break,
                    },
                    _ = tick.tick() => {}
                }
                let Some(me) = weak.upgrade() else { break };
                me.run_schedules(chrono::Local::now().naive_local()).await;
                me.fill();
            }
        });
        me
    }

    /// Queues that just finished, with what should happen now.
    pub fn subscribe_finished(&self) -> broadcast::Receiver<(Queue, WhenDone)> {
        self.finished.subscribe()
    }

    /// Sets or clears a queue's schedule.
    pub fn set_schedule(&self, id: i64, schedule: Option<Schedule>) -> Result<Queue, String> {
        if let Some(s) = &schedule {
            s.validate()?;
        }
        self.edit(|list| {
            let q = list
                .iter_mut()
                .find(|q| q.id == id)
                .ok_or("no such queue")?;
            q.schedule = schedule;
            Ok(q.clone())
        })
    }

    /// Starts and stops queues whose scheduled time is `now` (local time), and reports
    /// queues that have run out of work. Public so tests can pass any time.
    pub async fn run_schedules(&self, now: chrono::NaiveDateTime) {
        use chrono::{Datelike, Timelike};
        let minute_key = now.and_utc().timestamp() / 60;
        let now_min = now.hour() * 60 + now.minute();
        let today = now.weekday().num_days_from_monday() as u8;
        for q in self.list() {
            let Some(s) = &q.schedule else { continue };
            if !s.days.is_empty() && !s.days.contains(&today) {
                continue;
            }
            for (is_start, time) in [(true, &s.start), (false, &s.stop)] {
                let Some(t) = time.as_deref().and_then(minutes) else {
                    continue;
                };
                if t != now_min {
                    continue;
                }
                let previous = self
                    .fired
                    .lock()
                    .unwrap()
                    .insert((q.id, is_start), minute_key);
                if previous == Some(minute_key) {
                    continue;
                }
                if is_start && !q.running {
                    let _ = self.start_queue(q.id);
                } else if !is_start && q.running {
                    let _ = self.stop_queue(q.id).await;
                }
            }
        }
        self.check_finished();
    }

    /// A running queue with nothing waiting or downloading is finished: stop it and say so.
    fn check_finished(&self) {
        let all = self.engine.list();
        for q in self.list().into_iter().filter(|q| q.running) {
            let busy = all
                .iter()
                .filter(|d| d.queue_id == Some(q.id))
                .any(|d| d.status == DownloadStatus::Queued || self.engine.is_running(d.id));
            if busy {
                self.had_work.lock().unwrap().insert(q.id);
                continue;
            }
            // Only once it has done something (not when it was started empty).
            if !self.had_work.lock().unwrap().remove(&q.id) {
                continue;
            }
            let done = self.edit(|list| {
                let x = list.iter_mut().find(|x| x.id == q.id)?;
                x.running = false;
                Some(x.clone())
            });
            if let Some(done) = done {
                let action = done
                    .schedule
                    .as_ref()
                    .map(|s| s.when_done)
                    .unwrap_or_default();
                let _ = self.finished.send((done, action));
            }
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Vec<Queue>> {
        self.changed.subscribe()
    }

    pub fn list(&self) -> Vec<Queue> {
        self.list.lock().unwrap().clone()
    }

    fn save(&self, list: &[Queue]) {
        if let Ok(json) = serde_json::to_string(list) {
            let _ = self.engine.store_setting(KEY, &json);
        }
        let _ = self.changed.send(list.to_vec());
    }

    fn edit<T>(&self, f: impl FnOnce(&mut Vec<Queue>) -> T) -> T {
        let mut list = self.list.lock().unwrap();
        let out = f(&mut list);
        self.save(&list);
        out
    }

    pub fn create(&self, name: &str, max_active: u8) -> Queue {
        self.edit(|list| {
            let id = list.iter().map(|q| q.id).max().unwrap_or(0) + 1;
            let name = name.trim();
            let q = Queue {
                id,
                name: if name.is_empty() {
                    format!("Queue {id}")
                } else {
                    name.to_string()
                },
                max_active: max_active.clamp(1, 32),
                running: false,
                schedule: None,
            };
            list.push(q.clone());
            q
        })
    }

    pub fn update(
        &self,
        id: i64,
        name: Option<&str>,
        max_active: Option<u8>,
    ) -> Result<Queue, String> {
        self.edit(|list| {
            let q = list
                .iter_mut()
                .find(|q| q.id == id)
                .ok_or("no such queue")?;
            if let Some(n) = name.map(str::trim).filter(|n| !n.is_empty()) {
                q.name = n.to_string();
            }
            if let Some(m) = max_active {
                q.max_active = m.clamp(1, 32);
            }
            Ok(q.clone())
        })
    }

    /// Puts downloads in a queue (or takes them out with `None`).
    pub fn assign(&self, ids: &[i64], queue: Option<i64>) -> Result<(), String> {
        if let Some(q) = queue
            && !self.list.lock().unwrap().iter().any(|x| x.id == q)
        {
            return Err("no such queue".into());
        }
        for id in ids {
            self.engine
                .set_queue(*id, queue)
                .map_err(|e| e.to_string())?;
        }
        self.fill();
        Ok(())
    }

    /// Starts the queue. Its paused downloads become waiting again, so they get their turn.
    pub fn start_queue(&self, id: i64) -> Result<(), String> {
        if !self.list.lock().unwrap().iter().any(|q| q.id == id) {
            return Err("no such queue".to_string());
        }
        // Give its downloads their turn *before* it counts as running, so the
        // "finished" check never sees a running queue with nothing to do.
        self.had_work.lock().unwrap().remove(&id);
        for d in self
            .engine
            .list()
            .into_iter()
            .filter(|d| d.queue_id == Some(id))
        {
            let _ = self.engine.requeue(d.id);
        }
        self.edit(|list| {
            if let Some(q) = list.iter_mut().find(|q| q.id == id) {
                q.running = true;
            }
        });
        self.fill();
        Ok(())
    }

    /// Stops the queue and pauses the downloads it was running.
    pub async fn stop_queue(&self, id: i64) -> Result<(), String> {
        self.had_work.lock().unwrap().remove(&id);
        self.edit(|list| match list.iter_mut().find(|q| q.id == id) {
            Some(q) => {
                q.running = false;
                Ok(())
            }
            None => Err("no such queue".to_string()),
        })?;
        for d in self
            .engine
            .list()
            .into_iter()
            .filter(|d| d.queue_id == Some(id))
        {
            if self.engine.is_running(d.id) {
                let _ = self.engine.pause(d.id).await;
            }
        }
        Ok(())
    }

    /// Deletes a queue. Its downloads stay in the list (out of any queue) unless
    /// `remove_downloads`; `delete_files` also deletes their files from disk.
    pub async fn delete_queue(
        &self,
        id: i64,
        remove_downloads: bool,
        delete_files: bool,
    ) -> Result<(), String> {
        let existed = self.edit(|list| {
            let before = list.len();
            list.retain(|q| q.id != id);
            list.len() != before
        });
        if !existed {
            return Err("no such queue".into());
        }
        for d in self
            .engine
            .list()
            .into_iter()
            .filter(|d| d.queue_id == Some(id))
        {
            if remove_downloads {
                self.engine
                    .remove(d.id, delete_files)
                    .await
                    .map_err(|e| e.to_string())?;
            } else {
                if self.engine.is_running(d.id) {
                    let _ = self.engine.pause(d.id).await;
                }
                self.engine
                    .set_queue(d.id, None)
                    .map_err(|e| e.to_string())?;
            }
        }
        Ok(())
    }

    /// Keeps every running queue at its number of active downloads, oldest first.
    pub fn fill(&self) {
        let queues = self.list();
        if !queues.iter().any(|q| q.running) {
            return;
        }
        let all = self.engine.list();
        for q in queues.iter().filter(|q| q.running) {
            let mine: Vec<_> = all.iter().filter(|d| d.queue_id == Some(q.id)).collect();
            let active = mine.iter().filter(|d| self.engine.is_running(d.id)).count();
            let free = (q.max_active as usize).saturating_sub(active);
            let waiting = mine
                .iter()
                // Only waiting ones: a download the user paused stays paused.
                .filter(|d| d.status == DownloadStatus::Queued && !self.engine.is_running(d.id))
                .take(free);
            for d in waiting {
                let _ = self.engine.start(d.id);
            }
        }
    }
}
