//! Ready-made DynamoDB tables — table-level metadata only.
//!
//! DynamoDB control plane runs on JSON-1.0 (note the version: not 1.1).
//! [`tables_table`] enumerates the account/region's tables via
//! `ListTables`, which returns nothing but the table name per row.
//! Item-level scan/query is a different beast and isn't surfaced
//! here — `vantage-aws` is read-list-only in v0.

pub mod table;

pub use table::{DynamoDbTable, tables_table};
