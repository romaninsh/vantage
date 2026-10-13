//! Rhai DSL golden suite: every `tests/rhai-tests/*.rhai` script against its
//! pinned `.surql` output or `.err` message. Offline: scripts are rendered,
//! never sent to a database. A golden that no longer matches the renderer
//! fails here.

#![cfg(feature = "rhai")]

#[path = "support/rhai_golden.rs"]
mod golden;

#[test]
fn every_fixture_matches_its_golden() {
    let mismatches = golden::mismatches();
    assert!(mismatches.is_empty(), "{}", mismatches.join("\n\n"));
}
