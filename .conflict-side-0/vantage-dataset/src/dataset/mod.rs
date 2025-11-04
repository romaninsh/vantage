//! Datasets are like a `Vec<E>`, but E are stored remotely and only fetched when needed.
//!
//! When we operate with DataSet we do not know how many rows are in a DataSet.
//! An example dataset could contain all the Orders placed by a client.
//!
//! There are four main traits for Datasets:
//!  - [`ReadableDataSet`]: allows to read rows
//!  - [`InsertableDataSet`]: allows to insert new rows
//!  - [`WritableDataSet`]: allows to update or delete rows
//!  - [`DataSet`]: combines all three operations above
//!
//! The included implementation for Datasets are:
//!  - Table: a table is a dataset that stores data in a SQL table and implements both [`ReadableDataSet`] and [`WritableDataSet`].
//!  - Query: a generic SELECT query that can fetch data and therefore implements [`ReadableDataSet`].

use async_trait::async_trait;
use vantage_core::Entity;
pub use vantage_core::{Result, VantageError};

mod valueset;
pub use valueset::*;

#[allow(clippy::module_inception)]
mod dataset;
pub use dataset::*;

/// Trait for datasets that can import records from other datasets
#[async_trait]
pub trait Importable<T: Entity>: DataSet<T> {
    /// Import records from another dataset
    async fn import<D, T2>(&mut self, source: D) -> Result<()>
    where
        D: crate::dataset::ReadableDataSet<T2> + Send,
        T2: Entity,
        T: From<D>;
}
