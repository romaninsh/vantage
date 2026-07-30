//! The official per-datasource debug stream.
//!
//! A [`DebugTap`] is carried by every [`Lens`](crate::Lens) and reached from
//! every task the lens spawns. When enabled (one datasource opted in via
//! `debug: true`), curated events emit at `info` level under the single
//! target `vantage_diorama::debug` — visible in a default log with no
//! `RUST_LOG` required. When off, nothing is emitted and nothing is paid.
//!
//! This stream is the mechanism for demonstrating the cache's efficiency
//! and its resilience to backend faults: every master round trip, every
//! cache mutation, every consumer open/close (the *census*), every status
//! transition — attributable, correlated (`req=N`), and greppable.

use std::sync::Arc;
use std::sync::LazyLock;
use std::time::Instant;

/// Per-datasource debug switch. Cheap to clone, cheap to check.
#[derive(Debug, Clone, Default)]
pub struct DebugTap {
    /// `Some(name)` = enabled for that datasource; `None` = off.
    ds: Option<Arc<str>>,
}

impl DebugTap {
    /// The disabled tap — the default for every Lens.
    pub fn off() -> Self {
        Self { ds: None }
    }

    /// An enabled tap tagged with the datasource's name; every emitted
    /// line carries it as `ds=<name>`.
    pub fn for_datasource(name: impl Into<String>) -> Self {
        Self {
            ds: Some(Arc::from(name.into())),
        }
    }

    pub fn enabled(&self) -> bool {
        self.ds.is_some()
    }

    /// The datasource name, or `""` when the tap is off. Only meaningful
    /// inside a `tapline!` (which never fires when off).
    pub fn ds(&self) -> &str {
        self.ds.as_deref().unwrap_or("")
    }
}

/// Emit one debug-stream line, only when the tap is enabled.
///
/// `tapline!(tap, field = value, ..., "message")` — the target and the
/// `ds` field are supplied here so call sites can't drift.
#[allow(unused_macros)]
macro_rules! tapline {
    ($tap:expr, $($rest:tt)*) => {
        if $tap.enabled() {
            tracing::info!(target: "vantage_diorama::debug", ds = %$tap.ds(), $($rest)*);
        }
    };
}
#[allow(unused_imports)]
pub(crate) use tapline;

/// Wall/CPU/memory snapshot for census lines and the exit summary.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ProcessStats {
    /// Milliseconds since the first `process_stats()` call in this process.
    pub uptime_ms: u64,
    /// User + system CPU time consumed by the process, in milliseconds.
    pub cpu_ms: u64,
    /// Peak resident set size, in bytes. 0 where unsupported.
    pub peak_rss_bytes: u64,
}

static PROCESS_START: LazyLock<Instant> = LazyLock::new(Instant::now);

/// Snapshot process wall-clock, CPU time, and peak RSS.
///
/// Unix only (`getrusage`); other platforms report uptime and zeros.
pub fn process_stats() -> ProcessStats {
    let uptime_ms = PROCESS_START.elapsed().as_millis() as u64;
    #[cfg(unix)]
    {
        let mut usage: libc::rusage = unsafe { std::mem::zeroed() };
        if unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut usage) } == 0 {
            let tv_ms = |tv: libc::timeval| tv.tv_sec as u64 * 1000 + tv.tv_usec as u64 / 1000;
            // ru_maxrss is bytes on macOS, kilobytes on Linux.
            #[cfg(target_os = "macos")]
            let peak = usage.ru_maxrss as u64;
            #[cfg(not(target_os = "macos"))]
            let peak = usage.ru_maxrss as u64 * 1024;
            return ProcessStats {
                uptime_ms,
                cpu_ms: tv_ms(usage.ru_utime) + tv_ms(usage.ru_stime),
                peak_rss_bytes: peak,
            };
        }
    }
    ProcessStats {
        uptime_ms,
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tap_is_off_by_default_and_carries_the_datasource_name() {
        let off = DebugTap::default();
        assert!(!off.enabled());
        assert_eq!(off.ds(), "");
        let on = DebugTap::for_datasource("librarian");
        assert!(on.enabled());
        assert_eq!(on.ds(), "librarian");
    }

    #[test]
    fn process_stats_reports_nonzero_cpu_and_rss() {
        // Burn a little CPU so utime is measurable.
        let mut x = 0u64;
        for i in 0..5_000_000u64 {
            x = x.wrapping_add(i);
        }
        std::hint::black_box(x);
        let s = process_stats();
        #[cfg(unix)]
        {
            assert!(s.peak_rss_bytes > 0, "peak RSS should be measurable on unix");
            assert!(s.cpu_ms > 0, "cpu time should be nonzero after busy loop");
        }
        let _ = s.uptime_ms; // monotonic, may be 0 in a fast test — presence is enough
    }
}
