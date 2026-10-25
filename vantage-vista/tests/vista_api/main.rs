//! The `Vista` data model without scripting: queries, evaluation, specs,
//! references, catalogs and nested inserts, through the public API only.

#![cfg(feature = "mocks")]

use ciborium::Value as CborValue;
use vantage_types::Record;

mod catalog;
mod columns;
mod eval;
mod id_generator;
mod insert;
mod mini;
mod query;
mod reference;
mod spec;
mod vista;

fn t(s: &str) -> CborValue {
    CborValue::Text(s.into())
}

fn i(n: i64) -> CborValue {
    CborValue::Integer(n.into())
}

fn rec(pairs: &[(&str, CborValue)]) -> Record<CborValue> {
    pairs
        .iter()
        .map(|(k, v)| ((*k).into(), v.clone()))
        .collect()
}
