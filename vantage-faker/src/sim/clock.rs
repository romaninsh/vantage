//! Wall clock (real or manual) and each def's sim clock on top of it.

use std::time::{Instant, SystemTime, UNIX_EPOCH};

/// Where the engine reads the wall-clock time from.
#[derive(Clone, Copy, Debug)]
pub(super) enum Clock {
    /// The system clock, read as `wall` plus the monotonic time since
    /// `since`, so a wall-clock step cannot move the sims.
    System { wall: f64, since: Instant },
    /// A clock that stands still until [`SimEngine::advance`] moves it.
    ///
    /// [`SimEngine::advance`]: super::SimEngine::advance
    Manual(f64),
}

impl Clock {
    /// The system clock, starting now.
    pub fn system() -> Self {
        Self::System {
            wall: unix_secs(SystemTime::now()),
            since: Instant::now(),
        }
    }

    /// Wall-clock unix seconds.
    pub fn now(&self) -> f64 {
        match self {
            Self::System { wall, since } => wall + since.elapsed().as_secs_f64(),
            Self::Manual(t) => *t,
        }
    }
}

pub(super) fn unix_secs(t: SystemTime) -> f64 {
    t.duration_since(UNIX_EPOCH)
        .map_or(0.0, |d| d.as_secs_f64())
}

/// One def's sim clock: it meets the wall clock at `origin` (the engine
/// start) and runs `scale` times faster.
#[derive(Clone, Copy, Debug)]
pub(super) struct SimClock {
    pub origin: f64,
    pub scale: f64,
}

impl SimClock {
    /// Sim time at wall time `wall`.
    pub fn sim(&self, wall: f64) -> f64 {
        self.origin + (wall - self.origin) * self.scale
    }

    /// Wall time at sim time `sim`.
    pub fn wall(&self, sim: f64) -> f64 {
        self.origin + (sim - self.origin) / self.scale
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sim_and_wall_round_trip() {
        let c = SimClock {
            origin: 1000.0,
            scale: 10.0,
        };
        assert_eq!(c.sim(1010.0), 1100.0);
        assert_eq!(c.wall(1100.0), 1010.0);
        assert_eq!(c.sim(990.0), 900.0);
    }
}
