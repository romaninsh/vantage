//! Shared test harness for the Dio-query-semantics work.
//!
//! - [`MockView`] — a consumer that talks ONLY over a `TableScenery`, the way a
//!   real grid/dropdown does: drive the viewport, read `gray_rows`/`loaded_rows`/
//!   `is_loading`/`total`, and settle on generation bumps. No gpui, no UI types.
//! - bucket fixtures — an object-list master augmented with per-object detail,
//!   the canonical "cmd bucket of JSON" shape.
//! - Lens builders — [`memory_lens`], [`eager_dio`], [`paged_dio`] — so a test
//!   states only the loader it cares about.
//!
//! The root stays `support/mod.rs`: a `tests/support.rs` would be built as a
//! test binary of its own. Its groups live in children, re-exported here so
//! every importer reads them as `support::<item>`.

// Each test binary uses its own slice of the harness.
#![allow(dead_code, unused_imports)]

pub mod aggregate;
#[cfg(feature = "diagnostics")]
pub mod capture;
pub mod chunk;
pub mod flash;
pub mod spy_cache;
pub mod views;

mod lenses;
mod masters;
mod mock_view;
mod records;
mod waiting;

pub use lenses::*;
pub use masters::*;
pub use mock_view::*;
pub use records::*;
pub use waiting::*;
