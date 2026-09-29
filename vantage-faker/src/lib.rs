//! Synthetic data for Vantage.
//!
//! Two halves:
//! - [`ValueGen`] decides *what* a cell contains (name-aware, then type
//!   fallback), driven by explicit [`ColumnGen`] overrides where the default
//!   guess isn't enough. [`DatasetGen`] seeds a whole [`TableGen`] set into a
//!   [`vantage_memory::MemoryStore`], in reference order.
//! - the `sim` feature adds a Rhai-scripted [`SimEngine`] that mutates a
//!   seeded store live: insert/upsert/patch/set/delete against a run budget,
//!   ending in success once the ops run out or on the first failure.
//!
//! [`ShapedShell`] wraps a store-backed [`vantage_vista::source::TableShell`]
//! to make it behave like a real backend — paged or cursor-driven, sluggish
//! or flaky, honest or lying — for exercising a consumer against something
//! less forgiving than a plain in-memory table.

mod column;
#[cfg(feature = "serde")]
pub mod config;
pub mod dataset;
pub mod generator;
pub mod geo;
pub mod relational;
pub mod shape;
#[cfg(feature = "sim")]
pub mod sim;
pub mod value_gen;

pub use column::FakerColumn;
pub use dataset::{DatasetGen, TableGen};
pub use generator::{ColumnGen, Spread};
pub use relational::{FanOut, Reference, check_plan, relational_rows, seed_id};
pub use shape::{
    BackendShape, ExtraFields, FaultSchedule, Latency, LatencyModel, Offline, ShapedShell,
};
#[cfg(feature = "sim")]
pub use sim::{DEFAULT_OPS, SimDef, SimEngine, SimEngineBuilder, SimStats, Spawn};
pub use value_gen::ValueGen;
