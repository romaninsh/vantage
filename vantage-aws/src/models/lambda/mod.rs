//! Ready-made Lambda tables — functions, aliases, versions.
//!
//! Lambda speaks REST-JSON (see [`crate::restjson`]). The control
//! plane URL pattern is `/2015-03-31/functions/...` — old version
//! prefix, never going to change.
//!
//! Top-level: [`functions_table`] enumerates every function in the
//! account/region. Aliases and versions only make sense scoped to a
//! function, so they aren't exposed top-level — reach them via
//! `lambda.function ... :aliases` / `:versions`.
//!
//! `Function` also carries a cross-service `:log_group` relation that
//! resolves to the matching CloudWatch Logs group at
//! `/aws/lambda/<FunctionName>`, regardless of whether the function
//! has a custom logging config.

pub mod alias;
pub mod function;
pub mod version;

pub use alias::{Alias, aliases_table};
pub use function::{Function, functions_table};
pub use version::{Version, versions_table};
