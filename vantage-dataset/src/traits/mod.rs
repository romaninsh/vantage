//! Data/Value Sets are like a `Vec<E>`, but E are stored remotely and only fetched when needed.
//!
//! 1. When we operate with DataSet, we do not know how many rows are in it.
//! An example dataset could contain all the orders placed by a client.
//!
//! 2. Remotely stored data can be modified externally, for instance if multiple application
//! instances are operating in parallel.
//!
//! 3. Data loaded from DataSet may be incomplete - some columns may be inaccessible.
//!
//! 4. All records must have a unique ID, which will always point to the same record.
//!
//! Vantage provides [`DataSet<E>`] and [`ValueSet`] traits for a consistent interface.
//! There are three sub-traits for datasets:
//!  - [`ReadableDataSet`]: allows reading rows
//!  - [`InsertableDataSet`]: allows inserting new rows
//!  - [`WritableDataSet`]: allows updating or deleting rows
//!
//! For ValueSet there are also [`ReadableValueSet`], [`InsertableValueSet`] and [`WritableValueSet`].
//!
//! # Example: Time Travel Records
//!
//! ```rust,ignore
//! use vantage_dataset::prelude::*;
//! use serde::{Deserialize, Serialize};
//!
//! #[derive(Serialize, Deserialize, Clone)]
//! struct TimeTraveler {
//!     name: String,
//!     year_traveled: i32,
//!     destination: String,
//! }
//!
//! // Read time travel records from CSV file
//! let csv_data = CsvFile::<TimeTraveler>::new(csv_source, "time_travelers.csv");
//!
//! for (id, traveler) in csv_data.list().await? {
//!     println!("{} traveled to {} in {}",
//!         traveler.name, traveler.destination, traveler.year_traveled);
//! }
//! // Output: "Marty McFly traveled to 1885 in 2015"
//! ```
//!
//! `csv_data` implements ReadableDataSet and ReadableValueSet, meaning you can also use [`list_values`](ReadableValueSet::list_values)
//! as well as [`list`](ReadableDataSet::list) to get the records.

use async_trait::async_trait;
pub use vantage_core::{Result, VantageError};
use vantage_types::Entity;

mod valueset;
pub use valueset::*;

#[allow(clippy::module_inception)]
mod dataset;
pub use dataset::*;

/// Trait for datasets that can import records from other datasets
#[async_trait]
pub trait Importable<T>: DataSet<T>
where
    T: Entity<Self::Value>,
{
    /// Import records from another dataset
    async fn import<D, T2>(&mut self, source: D) -> Result<()>
    where
        D: crate::traits::ReadableDataSet<T2> + Send,
        T2: Entity<D::Value>,
        T: From<D>;
}
