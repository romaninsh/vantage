//! Trait impls for `AwsJson1`. Split per trait so each file is a
//! focused unit; the data shape and helpers stay in `super::AwsJson1`.

mod data_source;
mod expr_data_source;
mod table_source;
