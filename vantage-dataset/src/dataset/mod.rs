//! Datasets are like a `Vec<E>`, but E are stored remotely and only fetched when needed.
//!
//! When we operate with DataSet we do not know how many rows are in a DataSet.
//! An example dataset could contain all the Orders placed by a client.
//!
//! There are two main traits for Datasets:
//!  - [`ReadableDataSet`]: allows to read rows
//!  - [`WritableDataSet`]: allows to update or delete rows
//!
//! The included implementation for Datasets are:
//!  - Table: a table is a dataset that stores data in a SQL table and implements both [`ReadableDataSet`] and [`WritableDataSet`].
//!  - Query: a generic SELECT query that can fetch data and therefore implements [`ReadableDataSet`].
mod error;
pub use error::{DataSetError, Result};

/// Type alias for ID parameters that can be either &str or String
pub trait Id: Into<String> + Send {}
impl<T: Into<String> + Send> Id for T {}

mod insertable;
pub use insertable::{Importable, InsertableDataSet};

mod readable;
pub use readable::ReadableDataSet;

mod writable;
pub use writable::WritableDataSet;

// mod scalar;
// pub use scalar::ScalarDataSet;

mod candidate;
pub use candidate::*;
