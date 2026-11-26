//! Fetching a file over several connections at once.
//!
//! The file is cut into segments, one per connection. When a connection finishes its
//! segment it takes over the second half of the segment with the most bytes left, so
//! all connections stay busy until the very end. The connection that owned that
//! segment simply stops reading when it reaches the new, earlier end.

use std::fs::File;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures_util::StreamExt;
use reqwest::header::RANGE;
use reqwest::{Client, StatusCode};
use tokio::sync::Notify;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;

use crate::error::{Error, Result};
use crate::http;
use crate::limiter::Limiter;
use crate::model::{RequestInfo, Segment, SegmentView, UNKNOWN_END};

/// Bytes a connection collects before writing them to disk.
const WRITE_BUFFER: usize = 512 * 1024;

/// How many sections (connections) a running download should use. The user can change
/// it while the download runs: extra connections open at once, surplus ones close after
/// saving what they have, and their unfinished sections are picked up by the others.
#[derive(Debug)]
pub(crate) struct Sections {
    wanted: AtomicUsize,
    changed: Notify,
}

impl Sections {
    pub fn new(n: usize) -> Self {
        Self {
            wanted: AtomicUsize::new(n.max(1)),
            changed: Notify::new(),
        }
    }
    pub fn wanted(&self) -> usize {
        self.wanted.load(Ordering::SeqCst)
    }
    pub fn set(&self, n: usize) {
        self.wanted.store(n.max(1), Ordering::SeqCst);
        self.changed.notify_one();
    }
}

pub(crate) struct TransferOptions {
    /// A segment is only split when both halves would be at least this big.
    pub min_split: u64,
    /// Failed attempts in a row (without progress) before a connection gives up.
    pub max_retries: u32,
    pub retry_delay: Duration,
}

pub(crate) struct Transfer {
    pub client: Client,
    pub request: RequestInfo,
    pub url: String,
    pub resumable: bool,
    pub file: Arc<File>,
    pub limiters: Vec<Arc<Limiter>>,
    /// Cancelled when the user pauses, or when one connection hits an error that stops all.
    pub cancel: CancellationToken,
    pub options: TransferOptions,
    pub sections: Arc<Sections>,
    /// Connections currently running.
    pub live: AtomicUsize,
}

impl Transfer {
    fn wanted(&self) -> usize {
        if self.resumable {
            self.sections.wanted()
        } else {
            1
        }
    }

    /// Called by a connection between chunks: closes it if there are more than wanted.
    fn try_retire(&self) -> bool {
        let mut n = self.live.load(Ordering::SeqCst);
        loop {
            if n <= self.wanted() {
                return false;
            }
            match self
                .live
                .compare_exchange(n, n - 1, Ordering::SeqCst, Ordering::SeqCst)
            {
                Ok(_) => return true,
                Err(now) => n = now,
            }
        }
    }
}

struct SegState {
    seg: Segment,
    /// Bytes received, including ones still in a connection's write buffer (>= seg.pos).
    cursor: u64,
    active: bool,
}

pub(crate) struct Progress {
    segs: Mutex<Vec<SegState>>,
    received: AtomicU64,
}

impl Progress {
    pub fn new(segments: &[Segment]) -> Self {
        let received = segments.iter().map(Segment::done).sum();
        let segs = segments
            .iter()
            .map(|&seg| SegState {
                seg,
                cursor: seg.pos,
                active: false,
            })
            .collect();
        Self {
            segs: Mutex::new(segs),
            received: AtomicU64::new(received),
        }
    }

    /// Bytes received so far (some may still be in write buffers).
    pub fn received(&self) -> u64 {
        self.received.load(Ordering::Relaxed)
    }

    /// Segments as they are on disk, for saving.
    pub fn segments(&self) -> Vec<Segment> {
        self.segs.lock().unwrap().iter().map(|s| s.seg).collect()
    }

    /// Bytes on disk.
    pub fn written(&self) -> u64 {
        self.segs.lock().unwrap().iter().map(|s| s.seg.done()).sum()
    }

    pub fn views(&self) -> Vec<SegmentView> {
        self.segs
            .lock()
            .unwrap()
            .iter()
            .map(|s| SegmentView {
                start: s.seg.start,
                end: s.seg.end,
                pos: s.cursor,
                active: s.active,
            })
            .collect()
    }

    pub fn all_complete(&self) -> bool {
        self.segs
            .lock()
            .unwrap()
            .iter()
            .all(|s| s.seg.is_complete())
    }

    /// Finds work for a connection: an idle unfinished segment, or else half of the
    /// biggest running one.
    fn claim(&self, allow_split: bool, min_split: u64) -> Option<usize> {
        let mut segs = self.segs.lock().unwrap();
        if let Some(i) = segs.iter().position(|s| !s.active && s.cursor < s.seg.end) {
            segs[i].active = true;
            return Some(i);
        }
        if !allow_split {
            return None;
        }
        let (i, left) = segs
            .iter()
            .enumerate()
            .filter(|(_, s)| s.active && s.seg.end != UNKNOWN_END)
            .map(|(i, s)| (i, s.seg.end.saturating_sub(s.cursor)))
            .max_by_key(|&(_, left)| left)?;
        if left < min_split.saturating_mul(2) {
            return None;
        }
        let mid = segs[i].cursor + left / 2;
        let end = segs[i].seg.end;
        segs[i].seg.end = mid;
        segs.push(SegState {
            seg: Segment::new(mid, end),
            cursor: mid,
            active: true,
        });
        Some(segs.len() - 1)
    }

    fn release(&self, i: usize) {
        self.segs.lock().unwrap()[i].active = false;
    }

    /// Forgets bytes received but not written, before a retry.
    fn rewind(&self, i: usize) -> (u64, u64) {
        let mut segs = self.segs.lock().unwrap();
        let s = &mut segs[i];
        let lost = s.cursor - s.seg.pos;
        self.received.fetch_sub(lost, Ordering::Relaxed);
        s.cursor = s.seg.pos;
        (s.cursor, s.seg.end)
    }

    /// Starts a segment over from its first byte (servers that can't resume).
    fn restart(&self, i: usize) {
        let mut segs = self.segs.lock().unwrap();
        let s = &mut segs[i];
        self.received
            .fetch_sub(s.cursor - s.seg.start, Ordering::Relaxed);
        s.seg.pos = s.seg.start;
        s.cursor = s.seg.start;
    }

    /// Accepts up to `len` bytes for segment `i`. Returns how many fit and whether the segment is now full.
    fn accept(&self, i: usize, len: u64) -> (u64, bool) {
        let mut segs = self.segs.lock().unwrap();
        let s = &mut segs[i];
        let take = len.min(s.seg.end.saturating_sub(s.cursor));
        s.cursor += take;
        self.received.fetch_add(take, Ordering::Relaxed);
        (take, s.cursor >= s.seg.end)
    }

    fn mark_written(&self, i: usize, pos: u64) {
        self.segs.lock().unwrap()[i].seg.pos = pos;
    }

    /// The stream ended and the size was unknown: this is the end of the file.
    fn close_unknown(&self, i: usize) {
        let mut segs = self.segs.lock().unwrap();
        let s = &mut segs[i];
        s.seg.end = s.cursor;
    }
}

/// Runs all connections until the file is complete, an error stops them, or the user pauses.
pub(crate) async fn run(transfer: Arc<Transfer>, progress: Arc<Progress>) -> Result<()> {
    let mut set = JoinSet::new();
    let spawn = |set: &mut JoinSet<Result<()>>| {
        transfer.live.fetch_add(1, Ordering::SeqCst);
        set.spawn(worker(transfer.clone(), progress.clone()));
    };
    for _ in 0..transfer.wanted() {
        spawn(&mut set);
    }
    let mut first_err: Option<Error> = None;
    loop {
        tokio::select! {
            joined = set.join_next() => {
                let Some(joined) = joined else { break };
                let res = joined.unwrap_or_else(|e| Err(Error::Io(std::io::Error::other(e))));
                if let Err(e) = res {
                    // Keep the real cause, not the "cancelled" the other connections report after it.
                    let replace = match &first_err {
                        None => true,
                        Some(Error::Cancelled) => !matches!(e, Error::Cancelled),
                        Some(_) => false,
                    };
                    if replace {
                        first_err = Some(e);
                    }
                    transfer.cancel.cancel();
                }
            }
            // The user asked for more sections while downloading.
            _ = transfer.sections.changed.notified() => {
                while transfer.live.load(Ordering::SeqCst) < transfer.wanted()
                    && !transfer.cancel.is_cancelled()
                    && !progress.all_complete()
                {
                    spawn(&mut set);
                }
            }
        }
    }
    match first_err {
        Some(e) => Err(e),
        None => Ok(()),
    }
}

async fn worker(t: Arc<Transfer>, p: Arc<Progress>) -> Result<()> {
    let res = work(&t, &p).await;
    if !matches!(res, Ok(Exit::Retired)) {
        t.live.fetch_sub(1, Ordering::SeqCst);
    }
    res.map(|_| ())
}

enum Exit {
    /// Nothing left to fetch.
    Done,
    /// Closed because the user lowered the number of sections.
    Retired,
}

async fn work(t: &Transfer, p: &Progress) -> Result<Exit> {
    loop {
        if t.try_retire() {
            return Ok(Exit::Retired);
        }
        let Some(i) = p.claim(t.resumable, t.options.min_split) else {
            return Ok(Exit::Done);
        };
        let res = fetch_segment(t, p, i).await;
        p.release(i);
        if matches!(res?, Exit::Retired) {
            return Ok(Exit::Retired);
        }
    }
}

enum StreamEnd {
    Complete,
    /// The server closed the connection before the segment was done.
    Short,
    /// This connection closed itself to lower the section count.
    Retired,
}

async fn fetch_segment(t: &Transfer, p: &Progress, i: usize) -> Result<Exit> {
    let mut failures = 0u32;
    loop {
        if t.cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }
        let (from, to) = p.rewind(i);
        if from >= to {
            return Ok(Exit::Done);
        }
        let err = match stream_range(t, p, i, from, to).await {
            Ok(StreamEnd::Complete) => return Ok(Exit::Done),
            Ok(StreamEnd::Retired) => return Ok(Exit::Retired),
            Ok(StreamEnd::Short) => None,
            Err(Error::Cancelled) => return Err(Error::Cancelled),
            Err(e) if e.is_transient() => Some(e),
            Err(e) => return Err(e),
        };
        let progressed = p.segs.lock().unwrap()[i].seg.pos > from;
        failures = if progressed { 0 } else { failures + 1 };
        if failures > t.options.max_retries {
            return Err(err.unwrap_or(Error::Io(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "the server keeps closing the connection early",
            ))));
        }
        if let Some(e) = &err {
            tracing::debug!("segment {i}: retrying after error: {e}");
        }
        if !t.resumable {
            p.restart(i);
        }
        let delay = t.options.retry_delay * 2u32.saturating_pow(failures.saturating_sub(1).min(5));
        tokio::select! {
            _ = t.cancel.cancelled() => return Err(Error::Cancelled),
            _ = tokio::time::sleep(delay.min(Duration::from_secs(30))) => {}
        }
    }
}

async fn stream_range(
    t: &Transfer,
    p: &Progress,
    i: usize,
    from: u64,
    to: u64,
) -> Result<StreamEnd> {
    let mut rb = http::get(&t.client, &t.request, &t.url)?;
    if t.resumable {
        let range = if to == UNKNOWN_END {
            format!("bytes={from}-")
        } else {
            format!("bytes={from}-{}", to - 1)
        };
        rb = rb.header(RANGE, range);
    }
    let resp = tokio::select! {
        biased;
        _ = t.cancel.cancelled() => return Err(Error::Cancelled),
        r = rb.send() => r?,
    };
    http::check_status(&resp)?;
    if t.resumable {
        if resp.status() != StatusCode::PARTIAL_CONTENT {
            return Err(Error::FileChanged);
        }
        match http::parse_content_range(&resp) {
            Some((start, _)) if start == from => {}
            _ => return Err(Error::BadRange),
        }
    } else if from != 0 {
        return Err(Error::BadRange);
    }

    let mut stream = resp.bytes_stream();
    let mut buf: Vec<u8> = Vec::with_capacity(WRITE_BUFFER);
    let mut buf_start = from;
    let outcome = loop {
        let next = tokio::select! {
            biased;
            _ = t.cancel.cancelled() => break Err(Error::Cancelled),
            c = stream.next() => c,
        };
        let chunk = match next {
            None => {
                break Ok(if to == UNKNOWN_END {
                    StreamEnd::Complete
                } else {
                    StreamEnd::Short
                });
            }
            Some(Err(e)) => break Err(Error::from(e)),
            Some(Ok(c)) => c,
        };
        for l in &t.limiters {
            l.acquire(chunk.len() as u64).await;
        }
        let (take, full) = p.accept(i, chunk.len() as u64);
        buf.extend_from_slice(&chunk[..take as usize]);
        if (buf.len() >= WRITE_BUFFER || full)
            && let Err(e) = flush(t, p, i, &mut buf, &mut buf_start).await
        {
            break Err(e);
        }
        if full {
            break Ok(StreamEnd::Complete);
        }
        if t.try_retire() {
            break Ok(StreamEnd::Retired);
        }
    };
    // Whatever happened, keep the bytes we already have.
    flush(t, p, i, &mut buf, &mut buf_start).await?;
    if matches!(outcome, Ok(StreamEnd::Complete)) && to == UNKNOWN_END {
        p.close_unknown(i);
    }
    outcome
}

async fn flush(
    t: &Transfer,
    p: &Progress,
    i: usize,
    buf: &mut Vec<u8>,
    buf_start: &mut u64,
) -> Result<()> {
    if buf.is_empty() {
        return Ok(());
    }
    let data = std::mem::replace(buf, Vec::with_capacity(WRITE_BUFFER));
    let len = data.len() as u64;
    let file = t.file.clone();
    let offset = *buf_start;
    tokio::task::spawn_blocking(move || write_all_at(&file, &data, offset))
        .await
        .map_err(std::io::Error::other)??;
    *buf_start += len;
    p.mark_written(i, *buf_start);
    Ok(())
}

#[cfg(unix)]
fn write_all_at(file: &File, buf: &[u8], offset: u64) -> std::io::Result<()> {
    use std::os::unix::fs::FileExt;
    file.write_all_at(buf, offset)
}

#[cfg(windows)]
fn write_all_at(file: &File, mut buf: &[u8], mut offset: u64) -> std::io::Result<()> {
    use std::os::windows::fs::FileExt;
    while !buf.is_empty() {
        match file.seek_write(buf, offset) {
            Ok(0) => return Err(std::io::ErrorKind::WriteZero.into()),
            Ok(n) => {
                buf = &buf[n..];
                offset += n as u64;
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

/// The first cut of a file into `connections` equal parts, none smaller than `min_split`.
pub(crate) fn plan(size: u64, connections: usize, min_split: u64) -> Vec<Segment> {
    if size == 0 {
        return Vec::new();
    }
    let by_size = (size / min_split.max(1)).max(1) as usize;
    let n = connections.max(1).min(by_size) as u64;
    let part = size / n;
    (0..n)
        .map(|k| Segment::new(k * part, if k == n - 1 { size } else { (k + 1) * part }))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plans_even_parts() {
        let p = plan(100, 4, 10);
        assert_eq!(p.len(), 4);
        assert_eq!(p[0], Segment::new(0, 25));
        assert_eq!(p[3], Segment::new(75, 100));
        assert_eq!(plan(15, 8, 10).len(), 1);
        assert!(plan(0, 8, 10).is_empty());
        let odd = plan(103, 4, 1);
        assert_eq!(odd.last().unwrap().end, 103);
    }

    #[test]
    fn idle_connection_splits_biggest_segment() {
        let p = Progress::new(&[Segment::new(0, 1000), Segment::new(1000, 1100)]);
        assert_eq!(p.claim(true, 10), Some(0));
        assert_eq!(p.claim(true, 10), Some(1));
        p.accept(1, 100);
        p.release(1);
        p.accept(0, 200); // segment 0 has 800 left
        let new = p.claim(true, 10).unwrap();
        let segs = p.segments();
        assert_eq!(segs[0].end, 600);
        assert_eq!(segs[new], Segment::new(600, 1000));
        // Segment 0 owner can only take what's left before 600.
        assert_eq!(p.accept(0, 1000), (400, true));
    }

    #[test]
    fn no_split_below_minimum() {
        let p = Progress::new(&[Segment::new(0, 30)]);
        assert_eq!(p.claim(true, 10), Some(0));
        p.accept(0, 15);
        assert_eq!(p.claim(true, 10), None);
    }
}
