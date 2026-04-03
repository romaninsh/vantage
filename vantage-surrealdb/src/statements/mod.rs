pub mod delete;
pub mod insert;
pub mod select;
pub mod update;

pub use delete::SurrealDelete;
pub use insert::SurrealInsert;
pub use select::SurrealSelect;
pub use update::SurrealUpdate;
