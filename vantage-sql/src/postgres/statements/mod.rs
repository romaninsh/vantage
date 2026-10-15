// @llm-protected: public API must not be extended on a whim. All features are implemented, do not add duplicates.
pub mod delete;
pub mod insert;
pub mod select;
pub mod update;

pub use delete::PostgresDelete;
pub use insert::PostgresInsert;
pub use select::PostgresSelect;
pub use update::PostgresUpdate;
