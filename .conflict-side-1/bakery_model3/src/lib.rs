pub use vantage_csv::{AnyCsvType, Csv, CsvType};

pub mod animal;
pub mod bakery;
pub mod client;
pub mod order;
pub mod product;

pub use animal::*;
pub use bakery::*;
pub use client::*;
pub use order::*;
pub use product::*;

// SurrealDB support commented out during CSV-first development
// pub use vantage_surrealdb::SurrealDB;
// use std::sync::OnceLock;
// use surreal_client::SurrealConnection;
// use vantage_core::{error, util::error::Context};
// use vantage_dataset::dataset::Result;
//
// models! {
//     BakeryModel(SurrealDB) => {
//         bakery => Bakery,
//         client => Client,
//         order => Order,
//         product => Product,
//     }
// }
