//! Protocol-agnostic trait impls for `AwsAccount`. Routes through
//! `crate::dispatch` which picks the right wire protocol from the
//! table name's prefix; nothing in this module knows JSON-1.1 from
//! Query.

mod data_source;
mod expr_data_source;
mod table_source;
