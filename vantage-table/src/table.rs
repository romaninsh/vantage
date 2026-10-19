// @llm-protected: public API must not be extended on a whim. All features are implemented, do not add duplicates.
//! `Table<T, E>`: a record collection with columns, conditions, ordering and
//! pagination, over a [`TableSource`](crate::traits::table_source::TableSource)
//! `T` and an entity `E`. A source that also implements query or select
//! sources unlocks the extra methods under `impls`. Type erasure for
//! cross-driver work lives in `vantage-vista`.

pub mod base;
pub use base::*;

pub mod hooks;
pub use hooks::*;

pub mod audit;
pub use audit::*;

pub mod id_generator;
pub use id_generator::*;

pub mod inject;
pub use inject::*;

pub mod impls;
pub use impls::*;

mod row_fields;
pub use row_fields::retain_matching;

pub mod sets;
pub use sets::*;
