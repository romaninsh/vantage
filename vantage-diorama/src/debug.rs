//! The official per-datasource debug stream.
//!
//! A [`DebugTap`] is carried by every [`Lens`](crate::Lens) and reached from
//! every task the lens spawns. Enabled per datasource via
//! [`LensBuilder::debug_datasource`](crate::lens::LensBuilder::debug_datasource),
//! it emits at `info` level under the single target `vantage_diorama::debug`
//! — visible in a default log with no `RUST_LOG` required. Every line carries
//! `ds=<datasource>`; every per-dio line also carries `dio=<master table
//! name>`. Off — the default — nothing is emitted and nothing is paid: every
//! call site checks [`DebugTap::enabled`] before doing any work the line
//! itself needs, not just before formatting it.
//!
//! This stream is the mechanism for demonstrating the cache's efficiency
//! and its resilience to backend faults: every master round trip, every
//! cache mutation, every consumer open/close (the *census*), every status
//! transition — attributable, correlated (`req=N`), and greppable.
//!
//! # Line shape
//!
//! Every line is `<datasource>  <tag>  <clause>` — a scannable left edge and
//! one clause of plain English. Units are human (`3.0s`, `24KB`, `200,000`,
//! `0.1%`), and a field is omitted rather than printed empty.
//!
//! The **tag** is the grep anchor and comes from a closed set:
//!
//! | Tag | Says |
//! |---|---|
//! | `dio` | a Dio was created; a fetch was asked for, came back, or failed (`fetch #N`, `list #N`) |
//! | `source` | what the master can and cannot do, and how this view loads — once, at open |
//! | `scenery` | a view opened; a load-state transition; row positions dropped |
//! | `census` | a consumer attached or detached, with live counts and RSS |
//! | `viewport` | the range a consumer declared, and how many scroll events coalesced into it |
//! | `cache` | rows committed, or a viewport served locally with no fetch |
//! | `payload` | columns received against columns displayed, and the bytes |
//! | `total` | the grand total changed, and what decided it |
//! | `sort` / `search` | the query changed, and whether it was pushed to the source |
//! | `hydrate` | a two-pass detail sweep queued its pending ids |
//! | `derive` | a `vantage-diorama-aggregate` layer recomputed |
//! | `summary` | the end-of-session ledger (see [`stats::emit_debug_summary`](crate::stats::emit_debug_summary)) |
//!
//! `fetch #N` / `list #N` is a per-dio counter tying a request to its outcome;
//! it is allocated only when the tap is enabled. Lines from one load are not
//! emitted in a fixed order — the cache commit and the state transition happen
//! inside the operation the return line closes — so correlate on the id rather
//! than on adjacency.
//!
//! A `derive()`'s first load emits two `derive` lines: an eager compute that
//! seeds the derived Vista's schema, then the engine's own seed pass over the
//! same rows, which reports `unchanged` because it reads what the eager pass
//! just published. Both are real recomputations.

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

    /// An enabled tap tagged with the datasource's name; it prefixes every
    /// line the tap emits.
    pub fn for_datasource(name: impl Into<String>) -> Self {
        ANY_TAP_ENABLED.store(true, std::sync::atomic::Ordering::Relaxed);
        // Start the session clock here rather than at the first reader, so
        // "session" spans the whole of the debug stream instead of beginning
        // at whatever happened to look at it first.
        LazyLock::force(&PROCESS_START);
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
/// `tapline!(tap, "tag", "clause {}", value)` renders as
/// `<datasource>  <tag>  <clause>` — a scannable left edge (which source,
/// which kind of event) followed by one clause of plain English. The whole
/// invocation, arguments included, sits behind the enabled check, so a
/// disabled tap pays for nothing it would otherwise format.
macro_rules! tapline {
    ($tap:expr, $tag:literal, $($arg:tt)*) => {
        if $tap.enabled() {
            tracing::info!(
                target: "vantage_diorama::debug",
                "{:<10} {:<8} {}",
                $tap.ds(),
                $tag,
                format_args!($($arg)*),
            );
        }
    };
}
pub(crate) use tapline;

/// A duration in the unit a reader thinks in: `840ms`, `3.0s`, `1m12s`.
pub(crate) fn dur(ms: u64) -> String {
    if ms < 1_000 {
        format!("{ms}ms")
    } else if ms < 60_000 {
        format!("{:.1}s", ms as f64 / 1000.0)
    } else {
        format!("{}m{:02}s", ms / 60_000, (ms % 60_000) / 1000)
    }
}

/// A byte count as `812B`, `24KB`, `1.2MB`.
pub(crate) fn bytes(n: usize) -> String {
    const KB: usize = 1024;
    const MB: usize = KB * 1024;
    if n < KB {
        format!("{n}B")
    } else if n < MB {
        format!("{}KB", n / KB)
    } else {
        format!("{:.1}MB", n as f64 / MB as f64)
    }
}

/// A count with thousands separators — `200,000` reads, `200000` doesn't.
pub(crate) fn num(n: usize) -> String {
    let s = n.to_string();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// `held` of `total` as a percentage, precise enough to stay honest at the
/// small end: 200 of 200,000 is `0.1%`, not `0%`.
pub(crate) fn pct(held: usize, total: usize) -> String {
    if total == 0 {
        return "—".into();
    }
    let p = held as f64 / total as f64 * 100.0;
    if p >= 10.0 {
        format!("{p:.0}%")
    } else {
        format!("{p:.1}%")
    }
}

/// Wall/CPU/memory snapshot for census lines and the exit summary.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ProcessStats {
    /// Milliseconds since the debug stream was armed (the first
    /// [`DebugTap::for_datasource`]), not since process start — anything
    /// before the first datasource opted in is not measured.
    pub uptime_ms: u64,
    /// User + system CPU time consumed by the process, in milliseconds.
    pub cpu_ms: u64,
    /// Peak resident set size, in bytes. 0 where unsupported.
    pub peak_rss_bytes: u64,
}

/// Set the first time any datasource opts in. The exit summary consults it
/// so an embedder can call `emit_debug_summary()` unconditionally on quit
/// without printing a ledger nobody asked for.
static ANY_TAP_ENABLED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Whether any datasource enabled the debug stream in this process.
pub fn any_tap_enabled() -> bool {
    ANY_TAP_ENABLED.load(std::sync::atomic::Ordering::Relaxed)
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
            assert!(
                s.peak_rss_bytes > 0,
                "peak RSS should be measurable on unix"
            );
            assert!(s.cpu_ms > 0, "cpu time should be nonzero after busy loop");
        }
        let _ = s.uptime_ms; // monotonic, may be 0 in a fast test — presence is enough
    }
}
