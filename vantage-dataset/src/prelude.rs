//! Prelude module for vantage-dataset
//!
//! This module re-exports the most commonly used types and traits from vantage-dataset,
//! allowing users to import them with a single `use vantage_dataset::prelude::*;` statement.

// Dataset traits for working with data
pub use crate::dataset::{
    Id, Importable, InsertableDataSet, ReadableDataSet, Result, VantageError, WritableDataSet,
};

// DataSource traits for dataset discovery and creation
pub use crate::datasetsource::{
    DataSetSource, InsertableDataSetSource, ReadableDataSetSource, WritableDataSetSource,
};

// Re-export commonly used external dependencies
pub use async_trait::async_trait;
pub use serde::{Deserialize, Serialize};
