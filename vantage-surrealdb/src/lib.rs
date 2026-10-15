// @llm-protected: public API must not be extended on a whim. All features are implemented, do not add duplicates.
//! SurrealDB for Vantage: SurrealQL statement builders and primitives, the
//! `SurrealDB` datasource for typed tables, and (behind features) a Vista
//! driver and a Rhai query-building vocabulary.

pub mod ext;
pub mod field;
pub mod identifier;
pub mod macros;
pub mod operation;
pub mod primitives;
pub mod statements;
pub mod sum;
pub mod surreal_return;
pub mod surrealdb;
pub mod thing;
pub mod variable;

#[cfg(feature = "vista")]
pub mod vista;

#[cfg(feature = "rhai")]
pub mod rhai_engine;

pub use statements::SurrealDelete;
pub use statements::SurrealInsert;
pub use statements::SurrealSelect;
pub use statements::SurrealUpdate;

pub use statements::delete;
pub use statements::insert;
pub use statements::select;
pub use statements::update;

/// A SurrealQL expression over SurrealDB values.
pub type Expr = vantage_expressions::Expression<AnySurrealType>;

pub mod types;
pub use types::*;

pub use ext::SurrealTableExt;

pub use ciborium::Value as CborValue;
pub use types::{AnySurrealType, SurrealType, SurrealTypeVariants};
