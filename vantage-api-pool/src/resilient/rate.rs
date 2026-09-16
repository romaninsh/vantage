//! Token bucket: `per_second` tokens refill continuously up to `burst`;
//! each attempt takes one and waits when none is left.

use std::sync::Mutex;
use std::time::{Duration, Instant};

struct Level {
    tokens: f64,
    refilled_at: Instant,
}

pub(crate) struct TokenBucket {
    per_second: f64,
    burst: f64,
    level: Mutex<Level>,
}

impl TokenBucket {
    /// A `per_second` at or below zero is clamped to 0.001; a `burst` below
    /// 1 is clamped to 1.
    pub(crate) fn new(per_second: f64, burst: usize) -> Self {
        let burst = burst.max(1) as f64;
        Self {
            per_second: per_second.max(0.001),
            burst,
            level: Mutex::new(Level {
                tokens: burst,
                refilled_at: Instant::now(),
            }),
        }
    }

    /// Take one token, or return how long until one is available.
    fn try_take(&self) -> Option<Duration> {
        let mut level = self.level.lock().unwrap();
        let now = Instant::now();
        let refill = now.duration_since(level.refilled_at).as_secs_f64() * self.per_second;
        level.tokens = (level.tokens + refill).min(self.burst);
        level.refilled_at = now;
        if level.tokens >= 1.0 {
            level.tokens -= 1.0;
            return None;
        }
        Some(Duration::from_secs_f64(
            (1.0 - level.tokens) / self.per_second,
        ))
    }

    pub(crate) async fn acquire(&self) {
        while let Some(wait) = self.try_take() {
            tokio::time::sleep(wait).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn burst_then_wait() {
        let b = TokenBucket::new(1000.0, 2);
        assert_eq!(b.try_take(), None);
        assert_eq!(b.try_take(), None);
        let wait = b.try_take().expect("bucket empty");
        assert!(wait <= Duration::from_millis(1));
    }
}
