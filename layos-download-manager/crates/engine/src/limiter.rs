//! Token-bucket speed limiter. The rate can change while downloads run.

use std::sync::Mutex;
use std::time::{Duration, Instant};

#[derive(Debug)]
pub struct Limiter {
    state: Mutex<State>,
}

#[derive(Debug)]
struct State {
    /// Bytes per second, 0 = unlimited.
    rate: u64,
    /// May go negative: that's debt the next caller waits off.
    tokens: f64,
    last: Instant,
}

/// How much the bucket can hold, as a fraction of one second of the rate.
const BURST_SECONDS: f64 = 0.25;

impl Limiter {
    pub fn new(rate: u64) -> Self {
        Self {
            state: Mutex::new(State {
                rate,
                tokens: 0.0,
                last: Instant::now(),
            }),
        }
    }

    pub fn rate(&self) -> u64 {
        self.state.lock().unwrap().rate
    }

    pub fn set_rate(&self, rate: u64) {
        let mut s = self.state.lock().unwrap();
        s.rate = rate;
        s.tokens = 0.0;
        s.last = Instant::now();
    }

    /// How long the caller must wait after taking `bytes`. Zero when unlimited.
    fn take(&self, bytes: u64) -> Duration {
        let mut s = self.state.lock().unwrap();
        if s.rate == 0 {
            return Duration::ZERO;
        }
        let now = Instant::now();
        let rate = s.rate as f64;
        let elapsed = now.duration_since(s.last).as_secs_f64();
        s.last = now;
        s.tokens = (s.tokens + elapsed * rate).min(rate * BURST_SECONDS);
        s.tokens -= bytes as f64;
        if s.tokens >= 0.0 {
            Duration::ZERO
        } else {
            Duration::from_secs_f64(-s.tokens / rate)
        }
    }

    pub async fn acquire(&self, bytes: u64) {
        let wait = self.take(bytes);
        if !wait.is_zero() {
            tokio::time::sleep(wait).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn limits_rate() {
        let l = Limiter::new(100_000);
        let start = Instant::now();
        for _ in 0..20 {
            l.acquire(10_000).await; // 200 KB at 100 KB/s
        }
        let secs = start.elapsed().as_secs_f64();
        assert!(secs > 1.6 && secs < 2.6, "took {secs}");
    }

    #[tokio::test]
    async fn unlimited_never_waits() {
        let l = Limiter::new(0);
        let start = Instant::now();
        l.acquire(u32::MAX as u64).await;
        assert!(start.elapsed() < Duration::from_millis(50));
    }
}
