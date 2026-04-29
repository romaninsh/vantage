//! Custom value types for AWS records.
//!
//! - [`Arn`] — parsed Amazon Resource Name with multi-segment styled
//!   rendering.
//! - [`AwsDateTime`] — parsed AWS timestamp.
//! - [`AnyAwsType`] — polymorphic value enum used at the rendering
//!   boundary; built from a raw CBOR value plus the column's declared
//!   Rust type, so `with_column_of::<Arn>(...)` actually drives parsing.
//!
//! `AnyAwsType` is a presentation-layer concern — internal flow
//! (conditions, expressions, deferred resolution) keeps using
//! [`ciborium::Value`]. Convert at the boundary with
//! [`typed_records`] when you want rich rendering.

mod any;
mod arn;
mod datetime;

pub use any::AnyAwsType;
pub use arn::Arn;
pub use datetime::AwsDateTime;

use ciborium::Value as CborValue;
use indexmap::IndexMap;
use std::hash::Hash;
use vantage_types::Record;

/// Convert raw `Record<CborValue>`s into `Record<AnyAwsType>` using the
/// declared column types from `column_types` (column name → Rust type
/// name from `column.get_type()`).
///
/// Columns whose declared type matches a known wrapper (`Arn`,
/// `AwsDateTime`) get parsed into the typed variant; others fall back
/// to shape-based variants (`Text`, `Int`, …). Returns the records in
/// the same iteration order as the input.
pub fn typed_records<I>(
    records: IndexMap<I, Record<CborValue>>,
    column_types: &IndexMap<String, &'static str>,
) -> IndexMap<I, Record<AnyAwsType>>
where
    I: Eq + Hash,
{
    records
        .into_iter()
        .map(|(id, record)| {
            let typed: Record<AnyAwsType> = record
                .into_iter()
                .map(|(k, v)| {
                    let declared = column_types.get(&k).copied().unwrap_or("");
                    (k, AnyAwsType::from_cbor_typed(v, declared))
                })
                .collect();
            (id, typed)
        })
        .collect()
}

/// Untyped variant — useful when column type metadata isn't available
/// (e.g. records produced via `AnyTable::list_values`). Each value
/// gets shape-based variant selection; ARNs and dates aren't parsed.
pub fn untyped_records<I>(
    records: IndexMap<I, Record<CborValue>>,
) -> IndexMap<I, Record<AnyAwsType>>
where
    I: Eq + Hash,
{
    records
        .into_iter()
        .map(|(id, record)| {
            let typed: Record<AnyAwsType> = record
                .into_iter()
                .map(|(k, v)| (k, AnyAwsType::from_cbor_untyped(v)))
                .collect();
            (id, typed)
        })
        .collect()
}
