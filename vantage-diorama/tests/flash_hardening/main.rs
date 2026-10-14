//! Hardening of the flash pipeline:
//!
//! - **Effective write capabilities** — `dio.write_capabilities()` is the
//!   one gate UI chrome asks. Master caps by default; a Lens `writer` makes
//!   the Dio writable regardless of the master (a read-only CSV becomes
//!   editable, changes landing wherever the writer sends them).
//! - **Reconcile-while-pending** — a refresh carrying a master snapshot that
//!   predates an in-flight flash must not clobber it: `dio.reconcile()` skips
//!   rows with a flash in flight, and a confirmed flash re-asserts its fields
//!   over any stale write that raced it.
//! - **One write at a time** — the write queue runs a Dio's write-throughs
//!   in order, never two at once.
//! - **Drain, not drop** — flashes already in the queue keep the pipeline
//!   alive: dropping every external handle, and every waiting caller, still
//!   lands them.

mod capabilities;
#[cfg(feature = "test-util")]
mod drain;
mod one_at_a_time;
mod pending;
#[path = "../support/mod.rs"]
mod support;

use std::sync::Arc;

use vantage_diorama::{Cache, Lens};
use vantage_vista::{Column, Vista, VistaCapabilities, VistaMetadata, mocks::MockShell};

use support::record;

fn metadata() -> VistaMetadata {
    VistaMetadata::new()
        .with_column(Column::new("id", "String").with_flag("id"))
        .with_column(Column::new("name", "String").with_flag("title"))
        .with_column(Column::new("status", "String"))
        .with_id_column("id")
}

/// A master that can be read but not written — the mock analogue of a CSV
/// file or a third-party read-only API.
fn read_only_shell() -> MockShell {
    MockShell::new()
        .with_capabilities(VistaCapabilities::new())
        .with_record(
            "c1",
            record(&[("id", "c1"), ("name", "Ada"), ("status", "lead")]),
        )
}

fn vista_over(shell: &MockShell, name: &str) -> Vista {
    Vista::new(name, shell.clone().with_metadata(metadata()))
}

fn memory_lens() -> Arc<Lens> {
    Lens::builder()
        .cache(Cache::memory())
        .build()
        .expect("lens")
}

fn items() -> MockShell {
    MockShell::new()
        .with_record(
            "a",
            record(&[("id", "a"), ("name", "one"), ("status", "open")]),
        )
        .with_record("b", record(&[("id", "b"), ("name", "two")]))
}
