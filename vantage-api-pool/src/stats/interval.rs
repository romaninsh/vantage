use std::{
    cmp::min,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use rust_decimal::Decimal;

use super::Average;

#[derive(Debug, Clone, Copy)]
pub enum StatPeriod {
    Live(Instant),              // Stats being collected since the beginning
    Interval(Instant, Instant), // Stats within specific interval, but no longer actively collected
}

impl Default for StatPeriod {
    fn default() -> Self {
        StatPeriod::Live(Instant::now())
    }
}

impl StatPeriod {
    pub fn period(&self) -> Duration {
        match self {
            StatPeriod::Live(start) => Instant::now().duration_since(*start),
            StatPeriod::Interval(start, end) => end.duration_since(*start),
        }
    }

    pub fn is_global(&self) -> bool {
        matches!(self, StatPeriod::Live(_))
    }

    pub fn is_period(&self) -> bool {
        matches!(self, StatPeriod::Interval(_, _))
    }

    pub fn start(&self) -> Instant {
        match self {
            StatPeriod::Live(start) => *start,
            StatPeriod::Interval(start, _) => *start,
        }
    }

    pub fn end(&self) -> Instant {
        match self {
            StatPeriod::Live(_) => Instant::now(),
            StatPeriod::Interval(_, end) => *end,
        }
    }

    fn format_duration(from: &Instant, to: &Instant) -> String {
        let duration = to.duration_since(*from);
        let secs = duration.as_secs();
        let hours = secs / 3600;
        let minutes = (secs % 3600) / 60;
        let seconds = secs % 60;

        if hours > 0 {
            format!("{}h {}m {}s", hours, minutes, seconds)
        } else if minutes > 0 {
            format!("{}m {}s", minutes, seconds)
        } else {
            format!("{}s", seconds)
        }
    }

    fn format_instant(instant: &Instant) -> String {
        // Convert Instant to SystemTime
        let system_now = SystemTime::now();
        let instant_now = Instant::now();

        let duration_since_instant = instant_now.duration_since(*instant);
        let system_time = system_now - duration_since_instant;

        // Get seconds since UNIX epoch
        let duration_since_epoch = system_time
            .duration_since(UNIX_EPOCH)
            .expect("Time went backwards");

        let total_secs = duration_since_epoch.as_secs();

        // Calculate time components (UTC)
        let seconds_in_day = total_secs % 86400;
        let hours = seconds_in_day / 3600;
        let minutes = (seconds_in_day % 3600) / 60;
        let seconds = seconds_in_day % 60;

        format!("{:02}:{:02}:{:02}", hours, minutes, seconds)
    }

    pub fn format(&self) -> String {
        match self {
            StatPeriod::Live(start) => {
                format!(
                    "[last {}]",
                    StatPeriod::format_duration(start, &Instant::now())
                )
            }
            StatPeriod::Interval(start, end) => {
                format!(
                    "[{} +{}]",
                    StatPeriod::format_instant(start),
                    StatPeriod::format_duration(start, end)
                )
            }
        }
    }

    pub fn format_rps(&self, count: usize) -> String {
        let duration = self.period();
        let secs = duration.as_secs_f64();
        if secs > 0.0 {
            let rps = count as f64 / secs;
            format!("{:.2} rps", rps)
        } else {
            "N/A".to_string()
        }
    }
}

#[derive(Debug, Clone, Default, Copy)]
pub struct Stats {
    pub period: StatPeriod,

    pub success: usize,
    pub retries: usize,
    pub errors: usize,

    pub average_latency: Average,
}

impl Stats {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn empty_interval() -> Self {
        let now = Instant::now();
        Self {
            period: StatPeriod::Interval(now, now),
            ..Default::default()
        }
    }

    pub fn sleep_secs(&mut self, secs: u64) -> Duration {
        // sleep until self.period.start + duration, but not longer than duration
        let target_time = min(self.period.end(), Instant::now()) + Duration::from_secs(secs);
        if target_time < Instant::now() {
            Duration::ZERO
        } else {
            target_time.duration_since(Instant::now())
        }
    }

    pub fn snapshot(&self) -> Stats {
        if self.period.is_period() {
            return *self;
        }

        Stats {
            period: StatPeriod::Interval(self.period.start(), Instant::now()),
            success: self.success,
            retries: self.retries,
            errors: self.errors,
            average_latency: self.average_latency,
        }
    }

    // successful request
    pub fn success(&mut self, latency: Decimal) {
        self.success += 1;
        self.average_latency.add_sample(latency);
    }

    // retried request
    pub fn retry(&mut self) {
        self.retries += 1;
    }

    // errored request
    pub fn error(&mut self) {
        self.errors += 1;
    }

    pub fn get_total_requests(&self) -> usize {
        self.success + self.retries + self.errors
    }

    pub fn get_success(&self) -> usize {
        self.success
    }

    pub fn get_retries(&self) -> usize {
        self.retries
    }

    pub fn get_errors(&self) -> usize {
        self.errors
    }

    pub fn format(&self) -> String {
        let mut str = self.period.format();

        str.push_str(&format!(
            " {} requests ({})",
            self.success,
            self.period.format_rps(self.success)
        ));

        if self.success > 0 {
            str.push_str(&format!(
                ", avg latency: {:.2} ms",
                self.average_latency.get_value()
            ));
        }

        if self.retries > 0 {
            str.push_str(&format!(", {} retries", self.retries));
        }

        if self.errors > 0 {
            str.push_str(&format!(", {} errors", self.errors));
        }

        str
    }

    pub fn format_notime(&self) -> String {
        let mut str = String::new();

        str.push_str(&format!("{} requests", self.success,));

        if self.success > 0 {
            str.push_str(&format!(
                ", avg latency: {:.2} ms",
                self.average_latency.get_value()
            ));
        }
        if self.retries > 0 {
            str.push_str(&format!(", {} retries", self.retries));
        }

        if self.errors > 0 {
            str.push_str(&format!(", {} errors", self.errors));
        }

        str
    }
}

impl std::ops::Add for Stats {
    type Output = Stats;

    fn add(self, other: Stats) -> Stats {
        let period = StatPeriod::Interval(
            self.period.start(),
            self.period.end() + other.period.end().duration_since(other.period.start()),
        );

        // Preserve total duration and start time

        Stats {
            period,

            success: self.success + other.success,
            retries: self.retries + other.retries,
            errors: self.errors + other.errors,

            average_latency: self.average_latency + other.average_latency,
        }
    }
}

impl std::ops::Sub for Stats {
    type Output = Stats;

    fn sub(self, other: Stats) -> Stats {
        let period = StatPeriod::Interval(
            self.period.start() + other.period.end().duration_since(other.period.start()),
            self.period.end(),
        );

        Stats {
            period,

            success: self.success.saturating_sub(other.success),
            retries: self.retries.saturating_sub(other.retries),
            errors: self.errors.saturating_sub(other.errors),

            average_latency: self.average_latency - other.average_latency,
        }
    }
}

#[cfg(test)]
mod tests {
    use rust_decimal::dec;

    use super::*;
    #[test]
    fn test_stat_period() {
        // start live stats
        let mut stats = Stats::new();

        // two requests arrive
        stats.success(dec!(0.1));
        stats.success(dec!(0.2));

        // and one retry
        stats.retry();

        // during first minute
        let minute1 = stats.snapshot();

        // another one during second minute
        stats.success(dec!(0.3));
        stats.error();
        stats.error();

        let minute2 = stats.snapshot() - minute1;

        assert_eq!(
            minute1.format_notime(),
            "2 requests, avg latency: 0.15 ms, 1 retries"
        );

        assert_eq!(
            minute2.format_notime(),
            "1 requests, avg latency: 0.30 ms, 2 errors"
        );
    }
}
