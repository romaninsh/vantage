// @llm-protected: public API must not be extended on a whim. All features are implemented, do not add duplicates.
//! Fast in-memory datasource for Vantage. See SPEC.md.

pub mod eval;
pub mod seed;
pub mod store;
mod tx;
pub mod typed;
pub mod types;
mod vista;

pub use eval::{MemoryCondition, Query};
pub use store::{
    MemoryChange, MemoryStore, MemoryTable, MemoryTableHandle, Row, TableDef, UpsertOutcome,
};
pub use typed::{MemoryDB, MemoryRollup, MemoryRollupExt, operation::MemoryOperation};
pub use types::{AnyMemoryType, MemoryType, MemoryTypeVariants};
pub use vista::{
    MemoryBlock, MemoryTableExtras, MemoryTableShell, MemoryVistaFactory, MemoryVistaSpec,
};

pub mod prelude {
    pub use crate::eval::MemoryCondition;
    pub use crate::typed::operation::MemoryOperation;
    pub use crate::typed::{MemoryDB, MemoryRollupExt};
    pub use crate::types::{AnyMemoryType, MemoryType, MemoryTypeVariants};
}
