// @llm-protected: public API must not be extended on a whim. All features are implemented, do not add duplicates.
//! The commonly used traits and types, for `use vantage_dataset::prelude::*;`.

pub use crate::traits::{
    ActiveEntitySet, Importable, InsertableDataSet, InsertableValueSet, ReadableDataSet,
    ReadableValueSet, Result, VantageError, WritableDataSet, WritableValueSet,
};

pub use crate::im::{ImDataSource, ImTable};

pub use crate::record::{ActiveEntity, ActiveRecord};

pub use async_trait::async_trait;
pub use serde::{Deserialize, Serialize};
