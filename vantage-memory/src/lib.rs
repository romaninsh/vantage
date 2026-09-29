//! Fast in-memory datasource for Vantage. See SPEC.md.

pub mod store;
pub mod types;

pub use store::{MemoryChange, MemoryStore, MemoryTable, MemoryTableHandle, Row, TableDef};
pub use types::{AnyMemoryType, MemoryType, MemoryTypeVariants};
