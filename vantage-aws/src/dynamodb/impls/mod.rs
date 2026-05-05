//! Trait implementations that make `DynamoDB` plug into Vantage.
//!
//! `DataSource` is the marker. `TableSource` is currently a skeleton —
//! every method returns `todo!()` per the docs4 step-5 advice; impls
//! land incrementally driven by tests.

mod data_source;
mod expr_data_source;
mod table_source;
