use rust_decimal::prelude::*;
use std::collections::hash_map::DefaultHasher;
use std::hash::Hasher;
use std::{
    collections::HashMap,
    hash::Hash,
    sync::Mutex,
    time::{Duration, Instant},
};

#[derive(Debug, Clone)]
pub struct RateLimitPolicy {
    pub name: String,
    pub quota: u32,
    pub window: u64,
    pub epoch_start: Instant,
}

impl RateLimitPolicy {
    pub fn new(name: String, quota: u32, window: u64) -> Self {
        Self {
            name,
            quota,
            window,
            epoch_start: Instant::now(),
        }
    }
}

#[derive(Debug)]
struct RateLimitBucket {
    remaining: u32,
    reset_time: Instant,
}

#[derive(Debug)]
pub struct RateLimitPolicyEnforcer<T: Eq + Hash> {
    policy: RateLimitPolicy,
    buckets: Mutex<HashMap<T, RateLimitBucket>>,
    call_count: std::sync::atomic::AtomicU64,
}

impl<T: Eq + Hash + Clone> RateLimitPolicyEnforcer<T> {
    pub fn new(policy: RateLimitPolicy) -> Self {
        Self {
            policy,
            buckets: Mutex::new(HashMap::new()),
            call_count: std::sync::atomic::AtomicU64::new(0),
        }
    }

    fn key_jitter(&self, key: &T) -> Duration {
        let mut hasher = DefaultHasher::new();
        key.hash(&mut hasher);
        let hash = hasher.finish();
        // Jitter is 0-10% of window to prevent thundering herd but keep reasonable bounds
        let max_jitter_ms = (self.policy.window * 100).max(1000); // max 10% or 1 second
        let jitter_ms = hash % max_jitter_ms;
        Duration::from_millis(jitter_ms)
    }

    fn next_window_boundary(&self, now: Instant) -> Instant {
        let time_since_epoch = now.duration_since(self.policy.epoch_start);
        let current_window_number = time_since_epoch.as_secs() / self.policy.window;
        let next_window_number = current_window_number + 1;
        self.policy.epoch_start + Duration::from_secs(next_window_number * self.policy.window)
    }

    fn get_or_reset_bucket(&self, key: T, now: Instant) -> (Duration, RateLimitHeaders) {
        let mut buckets = self.buckets.lock().unwrap();
        let jitter = self.key_jitter(&key);

        let bucket = buckets.entry(key.clone()).or_insert_with(|| {
            let next_window_boundary = self.next_window_boundary(now);
            RateLimitBucket {
                remaining: self.policy.quota,
                reset_time: next_window_boundary + jitter,
            }
        });

        // Reset if window expired
        if now >= bucket.reset_time {
            let next_window_boundary = self.next_window_boundary(now);
            bucket.remaining = self.policy.quota;
            bucket.reset_time = next_window_boundary + jitter;
        }

        let sleep_duration = if bucket.remaining == 0 {
            bucket.reset_time.saturating_duration_since(now)
        } else {
            Duration::ZERO
        };

        let headers = RateLimitHeaders {
            policy: self.policy.clone(),
            remaining: bucket.remaining,
            reset_seconds: bucket.reset_time.saturating_duration_since(now).as_secs() + 1,
        };

        (sleep_duration, headers)
    }

    pub fn get_sleep(&self, key: T) -> (Duration, RateLimitHeaders) {
        let now = Instant::now();
        self.get_or_reset_bucket(key, now)
    }

    pub fn get_sleep_and_update(&self, key: T) -> (Duration, RateLimitHeaders) {
        let now = Instant::now();
        let (sleep_duration, mut headers) = self.get_or_reset_bucket(key.clone(), now);

        // Update bucket
        let mut buckets = self.buckets.lock().unwrap();
        if let Some(bucket) = buckets.get_mut(&key) {
            if bucket.remaining > 0 {
                bucket.remaining -= 1;
                headers.remaining = bucket.remaining;
            }
        }

        // Cleanup old buckets every ~1000 calls
        if self
            .call_count
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            .is_multiple_of(1000)
        {
            let cutoff = now - Duration::from_secs(3600);
            buckets.retain(|_, bucket| bucket.reset_time > cutoff);
        }

        (sleep_duration, headers)
    }

    pub fn get_rate_limit_headers(&self, key: T) -> RateLimitHeaders {
        let now = Instant::now();
        self.get_or_reset_bucket(key, now).1
    }

    pub fn set_desired_rate(&self, _rate: Decimal) {
        // No-op for compatibility with KeyedRateLimiter
    }
}

#[derive(Debug, Clone)]
pub struct RateLimitHeaders {
    pub policy: RateLimitPolicy,
    pub remaining: u32,
    pub reset_seconds: u64,
}

impl RateLimitHeaders {
    pub fn rate_limit_policy_header(&self) -> String {
        format!(
            "\"{}\";q={};w={}",
            self.policy.name, self.policy.quota, self.policy.window
        )
    }

    pub fn rate_limit_header(&self) -> String {
        format!(
            "\"{}\";r={};t={}",
            self.policy.name, self.remaining, self.reset_seconds
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_independent_keys() {
        let policy = RateLimitPolicy::new("test".to_string(), 60, 60);
        let enforcer = RateLimitPolicyEnforcer::new(policy);

        let (sleep_a, _) = enforcer.get_sleep_and_update("a");
        assert_eq!(sleep_a, Duration::ZERO);

        let (sleep_b, _) = enforcer.get_sleep_and_update("b");
        assert_eq!(sleep_b, Duration::ZERO);

        let headers_a = enforcer.get_rate_limit_headers("a");
        assert_eq!(headers_a.remaining, 59);

        let headers_b = enforcer.get_rate_limit_headers("b");
        assert_eq!(headers_b.remaining, 59);
    }

    #[test]
    fn test_quota_exhaustion() {
        let policy = RateLimitPolicy::new("test".to_string(), 2, 60);
        let enforcer = RateLimitPolicyEnforcer::new(policy);

        let (sleep1, _) = enforcer.get_sleep_and_update(1);
        assert_eq!(sleep1, Duration::ZERO);

        let (sleep2, _) = enforcer.get_sleep_and_update(1);
        assert_eq!(sleep2, Duration::ZERO);

        let (sleep3, _) = enforcer.get_sleep_and_update(1);
        assert!(sleep3 > Duration::ZERO);
        // With jitter, sleep can be slightly longer than window
        assert!(sleep3 <= Duration::from_secs(66)); // 60s window + max 6s jitter (10%)
    }

    #[test]
    fn test_headers_format() {
        let policy = RateLimitPolicy::new("test".to_string(), 100, 60);
        let enforcer = RateLimitPolicyEnforcer::new(policy);

        let headers = enforcer.get_rate_limit_headers("key1");

        let policy_header = headers.rate_limit_policy_header();
        assert!(policy_header.contains("\"test\""));
        assert!(policy_header.contains("q=100"));
        assert!(policy_header.contains("w=60"));

        let limit_header = headers.rate_limit_header();
        assert!(limit_header.contains("\"test\""));
        assert!(limit_header.contains("r=100"));
    }

    #[test]
    fn test_jitter_prevents_thundering_herd() {
        let policy = RateLimitPolicy::new("test".to_string(), 1, 10);
        let enforcer = RateLimitPolicyEnforcer::new(policy);

        // Verify different keys get different jitter values
        let jitter1 = enforcer.key_jitter(&"key1");
        let jitter2 = enforcer.key_jitter(&"key2");
        let jitter3 = enforcer.key_jitter(&"key3");

        // Jitter should be deterministic per key but different across keys
        assert_eq!(jitter1, enforcer.key_jitter(&"key1")); // Same key = same jitter
        assert!(
            jitter1 != jitter2 || jitter1 != jitter3 || jitter2 != jitter3,
            "Different keys should get different jitter values"
        );

        // All jitter should be within bounds (0-10% of window = 0-1000ms for 10s window)
        assert!(jitter1 <= Duration::from_millis(1000));
        assert!(jitter2 <= Duration::from_millis(1000));
        assert!(jitter3 <= Duration::from_millis(1000));
    }
}
