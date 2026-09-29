//! Fast in-memory datasource for Vantage. See SPEC.md.

pub mod store;

pub use store::{MemoryChange, MemoryStore, MemoryTable, MemoryTableHandle, Row, TableDef};
