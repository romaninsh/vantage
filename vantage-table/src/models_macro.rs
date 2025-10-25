//! Macro for defining model enums and related functions
//!
//! This macro generates:
//! - Module declarations and re-exports
//! - An enum with variants for each model/table type
//! - A function to list model names
//! - A function to get a table by name
//!
//! ## Example
//!
//! ```ignore
//! use vantage_table::models;
//! use vantage_surrealdb::SurrealDB;
//!
//! models! {
//!     MyAppModels(SurrealDB) => {
//!         user => User,
//!         post => Post,
//!         comment => Comment,
//!     }
//! }
//! ```
//!
//! This generates:
//! - `pub mod user; pub use user::*;` (and same for post, comment)
//! - `enum MyAppModels { User(Table<SurrealDB, User>), ... }`
//! - `fn model_names() -> Vec<&'static str>`
//! - `fn get_table(name: &str) -> Result<MyAppModels>`

/// Macro to define models with associated table types
///
/// Generates module declarations, enum definition, and helper functions
/// for working with multiple table types in a type-safe way.
#[macro_export]
macro_rules! models {
    (
        $enum_name:ident($datasource:ty) => {
            $($mod_name:ident => $struct_name:ident),* $(,)?
        }
    ) => {
        // Module declarations and re-exports
        $(
            pub mod $mod_name;
            pub use $mod_name::*;
        )*

        // Enum definition
        pub enum $enum_name {
            $(
                $struct_name($crate::Table<$datasource, $struct_name>),
            )*
        }

        // Function to list all model names
        pub fn model_names() -> Vec<&'static str> {
            vec![
                $(
                    stringify!($mod_name),
                )*
            ]
        }

        // Function to get a table by name
        pub fn get_table(model: &str, db: $datasource) -> ::vantage_core::Result<$enum_name> {
            match model {
                $(
                    stringify!($mod_name) => Ok($enum_name::$struct_name($struct_name::table(db.clone()))),
                )*
                _ => Err(::vantage_core::VantageError::other(format!("Unknown model: {}", model))),
            }
        }

    };
}
