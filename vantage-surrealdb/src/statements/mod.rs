//! SurrealDB statement builders.
//!
//! Provides type-safe builders for the four core SurrealDB statements:
//!
//! - [`SurrealSelect`] — `SELECT` queries with fields, conditions, ordering, grouping, limits
//! - [`SurrealInsert`] — `CREATE` statements with typed fields and optional record IDs
//! - [`SurrealUpdate`] — `UPDATE` statements in `SET`, `CONTENT`, or `MERGE` mode
//! - [`SurrealDelete`] — `DELETE` statements targeting records or whole tables
//!
//! All builders implement [`Expressive<AnySurrealType>`](vantage_expressions::Expressive),
//! producing parameterized expressions safe for execution via
//! [`ExprDataSource::execute()`](vantage_expressions::ExprDataSource::execute).
//!
//! # Consistent API
//!
//! Insert, Update, and Delete share a common builder pattern:
//!
//! | Method | Insert | Update | Delete |
//! |---|---|---|---|
//! | `::new(target)` | table name | any `Expressive` | any `Expressive` |
//! | `::table(name)` | — | table name | table name |
//! | `.with_id(id)` | ✓ | — | — |
//! | `.with_field(k, v)` | ✓ | ✓ | — |
//! | `.with_any_field(k, v)` | ✓ | ✓ | — |
//! | `.with_record(rec)` | ✓ | ✓ | — |
//! | `.with_condition(expr)` | — | ✓ | ✓ |
//! | `.content()` / `.merge()` / `.set()` | — | ✓ | — |

pub mod delete;
pub mod insert;
pub mod select;
pub mod update;

pub use delete::SurrealDelete;
pub use insert::SurrealInsert;
pub use select::SurrealSelect;
pub use update::SurrealUpdate;
