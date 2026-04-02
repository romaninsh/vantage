//! # Vantage Core
//!
//! Core traits and types used throughout the Vantage framework.

pub mod util;

// use serde::{Deserialize, Serialize, de::DeserializeOwned};
pub use util::{Context, Result, VantageError};

// /// Entity trait for types that can be used with datasets
// ///
// /// Entities must be serializable, deserializable, and support basic operations
// /// required for dataset manipulation across different data sources.
// pub trait Entity:
//     serde::Serialize + DeserializeOwned + Default + Clone + Send + Sync + Sized + 'static
// {
// }

// /// Auto-implement Entity for all types that satisfy the required bounds
// impl<T> Entity for T where
//     T: Serialize + DeserializeOwned + Default + Clone + Send + Sync + Sized + 'static
// {
// }

// /// Implement EmptyEntity
// #[derive(Clone, Debug, Serialize, Deserialize, Default)]
// pub struct EmptyEntity {}
