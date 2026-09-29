//! Fast in-memory datasource for Vantage. See SPEC.md.

pub mod eval;
pub mod seed;
pub mod store;
pub mod typed;
pub mod types;
pub mod vista;

pub use eval::{MemoryCondition, Query};
pub use store::{MemoryChange, MemoryStore, MemoryTable, MemoryTableHandle, Row, TableDef};
pub use typed::{MemoryDB, operation::MemoryOperation};
pub use types::{AnyMemoryType, MemoryType, MemoryTypeVariants};
pub use vista::{
    MemoryTableShell,
    factory::{MemoryVistaFactory, MemoryVistaSpec},
};

pub mod prelude {
    pub use crate::eval::MemoryCondition;
    pub use crate::typed::MemoryDB;
    pub use crate::typed::operation::MemoryOperation;
    pub use crate::types::{AnyMemoryType, MemoryType, MemoryTypeVariants};
}
