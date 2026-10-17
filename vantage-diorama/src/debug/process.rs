//! What the process costs. The peak-RSS reader the census line uses compiles
//! in every build, though a census line is only emitted by an enabled tap; the
//! `diagnostics` feature adds the session clock and the full snapshot.

#[cfg(feature = "diagnostics")]
use std::sync::LazyLock;
#[cfg(feature = "diagnostics")]
use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(feature = "diagnostics")]
use std::time::Instant;

/// Wall/CPU/memory snapshot for census lines and the exit summary.
#[cfg(feature = "diagnostics")]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct ProcessStats {
    /// Milliseconds since the debug stream was armed (the first
    /// [`DebugTap::for_datasource`](super::DebugTap::for_datasource)), not
    /// since process start — anything before the first datasource opted in is
    /// not measured.
    pub uptime_ms: u64,
    /// User + system CPU time consumed by the process, in milliseconds.
    pub cpu_ms: u64,
    /// Peak resident set size, in bytes. 0 where unsupported.
    pub peak_rss_bytes: u64,
}

/// Set the first time any datasource opts in. The exit summary consults it
/// so an embedder can call `emit_debug_summary()` unconditionally on quit
/// without printing a ledger nobody asked for.
#[cfg(feature = "diagnostics")]
static ANY_TAP_ENABLED: AtomicBool = AtomicBool::new(false);

#[cfg(feature = "diagnostics")]
static PROCESS_START: LazyLock<Instant> = LazyLock::new(Instant::now);

/// Record that a datasource opted in, and start the session clock here rather
/// than at the first reader, so "session" spans the whole of the debug stream
/// instead of beginning at whatever happened to look at it first.
#[cfg(feature = "diagnostics")]
pub(super) fn arm() {
    ANY_TAP_ENABLED.store(true, Ordering::Relaxed);
    LazyLock::force(&PROCESS_START);
}

/// Whether any datasource enabled the debug stream in this process.
#[cfg(feature = "diagnostics")]
pub fn any_tap_enabled() -> bool {
    ANY_TAP_ENABLED.load(Ordering::Relaxed)
}

/// Snapshot process wall-clock, CPU time, and peak RSS.
///
/// Unix only (`getrusage`); other platforms report uptime and zeros.
#[cfg(feature = "diagnostics")]
pub fn process_stats() -> ProcessStats {
    let (cpu_ms, peak_rss_bytes) = rusage().unwrap_or_default();
    ProcessStats {
        uptime_ms: PROCESS_START.elapsed().as_millis() as u64,
        cpu_ms,
        peak_rss_bytes,
    }
}

/// Peak resident set size in bytes, 0 where unsupported.
pub(crate) fn peak_rss_bytes() -> u64 {
    rusage().map_or(0, |(_, peak)| peak)
}

/// `(cpu_ms, peak_rss_bytes)` from `getrusage`.
#[cfg(unix)]
fn rusage() -> Option<(u64, u64)> {
    let mut usage: libc::rusage = unsafe { std::mem::zeroed() };
    if unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut usage) } != 0 {
        return None;
    }
    let tv_ms = |tv: libc::timeval| tv.tv_sec as u64 * 1000 + tv.tv_usec as u64 / 1000;
    // ru_maxrss is bytes on macOS, kilobytes on Linux.
    #[cfg(target_os = "macos")]
    let peak = usage.ru_maxrss as u64;
    #[cfg(not(target_os = "macos"))]
    let peak = usage.ru_maxrss as u64 * 1024;
    Some((tv_ms(usage.ru_utime) + tv_ms(usage.ru_stime), peak))
}

#[cfg(not(unix))]
fn rusage() -> Option<(u64, u64)> {
    None
}
