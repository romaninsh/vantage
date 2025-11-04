//! Prelude module for vantage-dataset
//!
//! This module re-exports the most commonly used types and traits from vantage-dataset,
//! allowing users to import them with a single `use vantage_dataset::prelude::*;` statement.

// Dataset traits for working with data
pub use crate::dataset::{
    Importable, InsertableDataSet, ReadableDataSet, ReadableValueSet, Result, VantageError,
    WritableDataSet, WritableValueSet,
};

// DataSource traits for dataset discovery and creation
pub use crate::datasetsource::{
    DataSetSource, InsertableDataSetSource, ReadableDataSetSource, WritableDataSetSource,
};

pub use crate::im::{ImDataSource, ImTable};

// Record functionality
pub use crate::record::{Record, RecordValue};

// Re-export commonly used external dependencies
pub use async_trait::async_trait;
pub use serde::{Deserialize, Serialize};
