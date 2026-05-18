//! Shared harness used by the cucumber binary in `tests/bdd.rs`.
//!
//! Submodules are picked up because `tests/bdd.rs` declares `mod bdd_support`.
//! Step modules under [`steps`] self-register with cucumber's macro runtime
//! at compile time — declaring them here is enough.

#![allow(unused_imports)] // Phase-1 placeholders consumed in later phases

pub mod backend;
pub mod spies;
pub mod steps;
pub mod world;

pub use backend::{BackendKind, MasterRows, RowSpec};
pub use spies::Spies;
pub use world::{DioramaWorld, LensBuilderState, OnWriteMode};
