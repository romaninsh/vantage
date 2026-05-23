//! Shared harness used by the cucumber binary in `tests/bdd.rs`.
//!
//! Submodules are picked up because `tests/bdd.rs` declares `mod bdd_support`.
//! Step modules under [`steps`] self-register with cucumber's macro runtime
//! at compile time — declaring them here is enough. Steps import directly
//! from each submodule's path; no re-exports needed.

pub mod backend;
pub mod spies;
pub mod sqlite_runtime;
pub mod steps;
pub mod world;
