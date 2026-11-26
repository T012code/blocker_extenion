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
}

pub struct Queues {
    engine: Engine,
    list: Mutex<Vec<Queue>>,
    changed: broadcast::Sender<Vec<Queue>>,
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
        let me = Arc::new(Self {
            engine,
            list: Mutex::new(list),
            changed,
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
                me.fill();
            }
        });
        me
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
        self.edit(|list| match list.iter_mut().find(|q| q.id == id) {
            Some(q) => {
                q.running = true;
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
            let _ = self.engine.requeue(d.id);
        }
        self.fill();
        Ok(())
    }

    /// Stops the queue and pauses the downloads it was running.
    pub async fn stop_queue(&self, id: i64) -> Result<(), String> {
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
