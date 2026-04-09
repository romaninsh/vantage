//! # Vantage MongoDB Extension
//!
//! Persistence backend for MongoDB using the official `mongodb` crate.
//! Uses `bson::Bson` as the native value type.

pub mod types;

pub use types::{AnyMongoType, MongoType, MongoTypeVariants};
