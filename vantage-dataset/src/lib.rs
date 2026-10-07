#![doc = include_str!("../README.md")]

pub mod record;
pub mod traits;

#[cfg(feature = "contract")]
pub mod contract;
pub mod im;
pub mod invariants;
pub mod mocks;
pub mod prelude;

pub use im::{ImDataSource, ImTable};
pub use mocks::csv::{AnyCsvType, CsvType, CsvTypePersistence};
pub use record::ActiveEntity;
pub use traits::{
    ActiveRecordSet, DataSet, InsertableDataSet, InsertableValueSet, ReadableDataSet,
    ReadableValueSet, ValueSet, WritableDataSet, WritableValueSet,
};
