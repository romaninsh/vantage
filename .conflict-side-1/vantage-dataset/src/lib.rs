#![doc = include_str!("../README.md")]

pub mod traits;
// pub mod datasetsource;
pub mod record;

pub mod im;
pub mod mocks;
pub mod prelude;

pub use im::{ImDataSource, ImTable};
pub use mocks::csv::{AnyCsvType, CsvType, CsvTypePersistence};
pub use record::ActiveEntity;
pub use traits::{
    ActiveRecordSet, DataSet, InsertableDataSet, InsertableValueSet, ReadableDataSet,
    ReadableValueSet, ValueSet, WritableDataSet, WritableValueSet,
};
