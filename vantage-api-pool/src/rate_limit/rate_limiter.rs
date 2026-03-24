use std::time::{Duration, Instant};

use rust_decimal::prelude::*;

pub struct RateLimiter {
    delay: Duration,
    last_request: Instant, // time of last request
}

impl RateLimiter {
    pub fn set_desired_rate(&mut self, rate: Decimal) {
        let delay_secs = (Decimal::ONE / rate).to_f64().unwrap_or(0.0);
        self.delay = Duration::try_from_secs_f64(delay_secs).unwrap_or(Duration::ZERO);
    }

    pub fn get_sleep(&mut self) -> Duration {
        let now = Instant::now();

        // Actually - next request
        self.last_request = (self.last_request.min(now) + self.delay).max(now);
        self.last_request.saturating_duration_since(now)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_last_request_one_minute_ago() {
        let mut limiter = RateLimiter {
            delay: Default::default(), // 1 request per second
            last_request: Instant::now() - Duration::from_secs(60),
        };
        limiter.set_desired_rate(dec!(1.0));

        let sleep_time = limiter.get_sleep();

        // Should return zero since last request was 60 seconds ago (much more than 1 second delay)
        assert_eq!(sleep_time, Duration::ZERO);

        // last_request should be updated to now
        assert!(limiter.last_request <= Instant::now());
    }

    #[test]
    fn test_last_request_one_minute_in_future() {
        let now = Instant::now();
        let mut limiter = RateLimiter {
            delay: Default::default(), // 1 request per second
            last_request: now + Duration::from_secs(60),
        };
        limiter.set_desired_rate(dec!(1.0));

        let sleep_time = limiter.get_sleep();

        // Should return ~1 second (starting from now, not from future time)
        assert!(sleep_time >= Duration::from_millis(900));
        assert!(sleep_time <= Duration::from_millis(1100));

        // last_request should be set to approximately now + 1 second
        let expected = now + Duration::from_secs(1);
        assert!(limiter.last_request >= expected - Duration::from_millis(10));
        assert!(limiter.last_request <= expected + Duration::from_millis(10));
    }

    #[test]
    fn test_last_request_is_now() {
        let now = Instant::now();
        let mut limiter = RateLimiter {
            delay: Default::default(), // 1 request per second
            last_request: now,
        };
        limiter.set_desired_rate(dec!(1.0));

        let sleep_time = limiter.get_sleep();

        // Should return ~1 second
        assert!(sleep_time >= Duration::from_millis(900));
        assert!(sleep_time <= Duration::from_millis(1100));

        // last_request should be set to approximately now + 1 second
        let expected = now + Duration::from_secs(1);
        assert!(limiter.last_request >= expected - Duration::from_millis(10));
        assert!(limiter.last_request <= expected + Duration::from_millis(10));
    }
}
