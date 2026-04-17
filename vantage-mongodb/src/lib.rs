//! # Vantage MongoDB Extension
//!
//! Persistence backend for MongoDB using the official `mongodb` crate.
//! Uses `bson::Bson` as the native value type and `bson::Document` as
//! the condition type — no SQL expressions involved.

pub mod condition;
pub mod id;
pub mod mongodb;
pub mod operation;
pub mod prelude;
pub mod select;
pub mod types;

pub use condition::MongoCondition;
pub use id::MongoId;
pub use mongodb::MongoDB;
pub use select::MongoSelect;
pub use types::{AnyMongoType, MongoType, MongoTypeVariants};
