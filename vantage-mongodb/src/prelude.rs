//! Common imports for working with vantage-mongodb.
//!
//! ```
//! use vantage_mongodb::prelude::*;
//! ```

pub use vantage_core::{Context, Result as VantageResult, VantageError};
pub use vantage_dataset::{
    InsertableDataSet, ReadableDataSet, ReadableValueSet, WritableDataSet, WritableValueSet,
};
pub use vantage_expressions::{Expression, Expressive};
pub use vantage_table::any::AnyTable;
pub use vantage_table::column::core::Column;
pub use vantage_table::pagination::Pagination;
pub use vantage_table::table::Table;
pub use vantage_table::traits::table_like::TableLike;
pub use vantage_table::traits::table_source::TableSource;
pub use vantage_types::{Entity, entity};

pub use serde::{Deserialize, Serialize, de::DeserializeOwned};

pub use crate::condition::MongoCondition;
pub use crate::id::MongoId;
pub use crate::mongodb::MongoDB;
pub use crate::operation::MongoOperation;
pub use crate::select::MongoSelect;
pub use crate::types::{AnyMongoType, MongoType};
