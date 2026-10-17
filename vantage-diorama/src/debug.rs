//! The official per-datasource debug stream.
//!
//! A [`DebugTap`] rides every [`Lens`](crate::Lens). Enabled per datasource
//! with [`LensBuilder::debug_datasource`](crate::lens::LensBuilder::debug_datasource),
//! it emits `<datasource>  <tag>  <clause>` lines at `info` under the one
//! target `vantage_diorama::debug`, visible without `RUST_LOG`. Off, the
//! default, nothing is emitted and nothing is paid. The tags, the `fetch #N`
//! correlation and how to read a session are in the book's debug-stream
//! chapter (`optimising-diorama/debug-stream.md`). Turning the tap on, and this
//! module's public surface, need the `diagnostics` feature.

mod process;

use std::sync::Arc;

pub(crate) use process::peak_rss_bytes;
#[cfg(feature = "diagnostics")]
pub use process::{ProcessStats, any_tap_enabled, process_stats};

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
    #[cfg(feature = "diagnostics")]
    pub fn for_datasource(name: impl Into<String>) -> Self {
        process::arm();
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
pub fn num(n: usize) -> String {
    let s = n.to_string();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
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
