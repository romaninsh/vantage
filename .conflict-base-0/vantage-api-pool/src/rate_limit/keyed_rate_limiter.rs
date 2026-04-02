use std::{
    collections::HashMap,
    hash::Hash,
    sync::Mutex,
    time::{Duration, Instant},
};

use rust_decimal::prelude::*;

#[derive(Debug)]
pub struct KeyedRateLimiter<T: Eq + Hash> {
    limiters: Mutex<HashMap<T, Instant>>,
    delay: Mutex<Duration>,
}
impl<T: Eq + Hash> KeyedRateLimiter<T> {
    pub fn new(rate: Decimal) -> Self {
        let s = Self {
            limiters: Mutex::new(HashMap::new()),
            delay: Mutex::new(Duration::ZERO),
        };
        s.set_desired_rate(rate);
        s
    }
    pub fn set_desired_rate(&self, rate: Decimal) {
        let delay_secs = (Decimal::ONE / rate).to_f64().unwrap_or(0.0);
        *self.delay.lock().unwrap() =
            Duration::try_from_secs_f64(delay_secs).unwrap_or(Duration::ZERO);
    }

    pub fn get_sleep(&self, key: T) -> Duration {
        let now = Instant::now();
        let delay = *self.delay.lock().unwrap();
        let mut limiters = self.limiters.lock().unwrap();

        // Get or insert the limiter for this key
        let last_request = limiters.entry(key).or_insert(now - delay);

        // Calculate next request time
        let next_request = ((*last_request).min(now) + delay).max(now);

        next_request.saturating_duration_since(now)
    }

    pub fn get_sleep_and_update(&self, key: T) -> Duration {
        let now = Instant::now();
        let delay = *self.delay.lock().unwrap();
        let mut limiters = self.limiters.lock().unwrap();

        // Get or insert the limiter for this key
        let last_request = limiters.entry(key).or_insert(now - delay);

        // Calculate next request time
        let next_request = ((*last_request).min(now) + delay).max(now);

        let last_secs = last_request.elapsed().as_secs();
        let next_secs = next_request.elapsed().as_secs();

        let last_ten_minutes = last_secs / 600; // 600 seconds = 10 minutes
        let next_ten_minutes = next_secs / 600;

        *last_request = next_request;

        // now we can do a bit of cleanup once every 10 minutes
        if last_ten_minutes != next_ten_minutes {
            limiters.retain(|_, &mut last_request| {
                now.duration_since(last_request) < Duration::from_secs(60)
            });
        }

        next_request.saturating_duration_since(now)
    }
}

#[cfg(test)]
mod tests {
    use std::thread::sleep;

    use super::*;

    #[test]
    fn test_independent_keys() {
        let limiter = KeyedRateLimiter::new(dec!(1.0));

        // First request for key "a"
        let sleep_a1 = limiter.get_sleep_and_update("a");
        assert_eq!(sleep_a1, Duration::ZERO);

        // First request for key "b" should also be zero (independent)
        let sleep_b1 = limiter.get_sleep_and_update("b");
        assert_eq!(sleep_b1, Duration::ZERO);

        // Second request for key "a" should wait
        let sleep_a2 = limiter.get_sleep_and_update("a");
        assert!(sleep_a2 >= Duration::from_millis(900));
        assert!(sleep_a2 <= Duration::from_millis(1100));
    }

    #[test]
    fn test_consecutive_requests_same_key() {
        let limiter = KeyedRateLimiter::new(dec!(2.0));

        // First request
        let sleep1 = limiter.get_sleep_and_update(1);
        assert_eq!(sleep1, Duration::ZERO);

        // Second request immediately after
        let sleep2 = limiter.get_sleep_and_update(1);
        assert!(sleep2 >= Duration::from_millis(450));
        assert!(sleep2 <= Duration::from_millis(550));
        sleep(sleep2);

        // Third request immediately after
        let sleep3 = limiter.get_sleep_and_update(1);
        assert!(sleep3 >= Duration::from_millis(450));
        assert!(sleep3 <= Duration::from_millis(550));
    }
}
