//! Vista-backed model-driven CLI runner.
//!
//! Drives a [`Vista`] from positional argv tokens. The grammar covers
//! model selection, locator resolution, operator conditions, relation
//! traversal, sort/slice selectors, search, aggregates, column
//! overrides, and JSON-typed value escapes. Backend specifics (which
//! names map to which models, what locator schemes are recognised, how
//! records are rendered) are injected through [`ModelFactory`] and
//! [`Renderer`].
//!
//! ## Token forms
//!
//! - `users` / `user` — model name (plural = list mode, singular =
//!   single).
//! - `arn:…`, `user:abc123`, `urn:…` — locator; resolved via
//!   [`ModelFactory::for_locator`].
//! - `field=value`, `field="quoted text"` — eq filter; `id=` is sugar
//!   for "narrow to this record".
//! - `field=#json-literal` — JSON-typed value (`#true`, `#42`,
//!   `#"42 as string"`, `#null`, `#[1,2,3]`, `#{"k":"v"}`).
//! - `field:lt=value` etc. — operator conditions
//!   (`ne`, `lt`, `lte`, `gt`, `gte`, `like`, `in`); `field:null` /
//!   `field:notnull` are nullary.
//! - `[…]` — combined sort + slice selector (see [`token::Selector`]).
//!   `[5]` narrows to row 5; `[5:15]` slices; `[+name]` sorts ascending;
//!   `[-name]` descending; `[+name:0]` sorts then narrows.
//! - `:relation` — traverse a typed relation; allowed only from single
//!   mode.
//! - `=col1,col2,…` — override the rendered columns.
//! - `?keyword` / `?"two words"` — full-text-ish search.
//! - `@sum:field`, `@max:field`, `@min:field`, `@count` — terminal
//!   aggregates.
//!
//! Glued bracket suffixes are accepted on most tokens (`users[0]`,
//! `:rel[0]`, `field=v[0]`, `=col1[0]`).
//!
//! See `parse` for the full grammar and `run` for the dispatch.

pub mod factory;
pub mod parse;
pub mod run;
pub mod token;
pub mod value;

pub use factory::{ModelFactory, Renderer};
pub use parse::{parse_selector, parse_token, split_bracket_suffix};
pub use run::run;
pub use token::{AggregateOp, Direction, Mode, Op, Selector, Slice, Token};
pub use value::{auto_detect, json_to_cbor, parse_value, parse_value_list};
