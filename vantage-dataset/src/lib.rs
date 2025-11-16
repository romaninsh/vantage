#![doc = include_str!("../README.md")]

pub mod traits;
// pub mod datasetsource;
pub mod record;

pub mod im;
pub mod prelude;

pub use im::{ImDataSource, ImTable};
pub use record::RecordEntity;
pub use traits::{
    DataSet, InsertableDataSet, InsertableValueSet, ReadableDataSet, ReadableValueSet,
    RecordValueSet, ValueSet, WritableDataSet, WritableValueSet,
};
