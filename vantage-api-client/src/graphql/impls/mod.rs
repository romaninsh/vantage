//! Trait impls that plug `GraphqlApi` into the Vantage query pipeline,
//! and the read paths behind them.

mod count;
mod data_source;
mod expr_data_source;
pub(crate) mod get_by_id;
mod local_filter;
pub(crate) mod search;
mod selectable_data_source;
pub(crate) mod table_source;
