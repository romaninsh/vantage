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
//!
//! # Feature
//!
//! The tap is compiled into every build, because every `tapline!` site calls
//! it, and it stays off unless a Lens enables it. Turning it on, and this
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
///
/// Public because the stream is a shared format: a crate that writes into
/// `vantage_diorama::debug` — `vantage-diorama-aggregate` does — has to print
/// its numbers the same way, or the reader meets two conventions in one log.
pub fn dur(ms: u64) -> String {
    if ms < 1_000 {
        format!("{ms}ms")
    } else if ms < 60_000 {
        format!("{:.1}s", ms as f64 / 1000.0)
    } else {
        format!("{}m{:02}s", ms / 60_000, (ms % 60_000) / 1000)
    }
}

/// A byte count as `812B`, `24KB`, `1.2MB`.
pub fn bytes(n: usize) -> String {
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
pub fn pct(held: usize, total: usize) -> String {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tap_is_off_by_default() {
        let off = DebugTap::default();
        assert!(!off.enabled());
        assert_eq!(off.ds(), "");
    }

    #[cfg(feature = "diagnostics")]
    #[test]
    fn an_enabled_tap_carries_the_datasource_name() {
        let on = DebugTap::for_datasource("librarian");
        assert!(on.enabled());
        assert_eq!(on.ds(), "librarian");
    }
}
