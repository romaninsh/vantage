//! Filter documents for the vista source's `add_condition`, and the downcast
//! behind `add_native_condition`.
//!
//! Operands that look like ids are matched in both spellings (`ObjectId` and
//! its 24-char hex string), because a text-typed surface cannot tell which one
//! the collection stores.

use bson::{Bson, Document, doc};
use vantage_core::{Result, error};
use vantage_vista::{Capability, FilterOp, NativeCondition, unsupported};

use crate::condition::MongoCondition;
use crate::id::{eq_filter, id_variants};
use crate::mongodb::impls::table_source::regex_escape;

/// `value` followed by its id counterpart, when it has one.
fn with_counterpart(value: &Bson) -> Vec<Bson> {
    let mut out = vec![value.clone()];
    out.extend(id_variants(value));
    out
}

/// Every element of `items`, each followed by its id counterpart.
fn expand_all(items: &[Bson]) -> Vec<Bson> {
    items.iter().flat_map(with_counterpart).collect()
}

/// The filter document for `path <op> value`.
pub(crate) fn op_filter(path: &str, op: FilterOp, value: &Bson) -> Result<Document> {
    let filter = match op {
        FilterOp::Eq => eq_filter(path, value),
        FilterOp::Ne => match id_variants(value) {
            Some(alt) => doc! { path: { "$nin": [value.clone(), alt] } },
            None => doc! { path: { "$ne": value.clone() } },
        },
        FilterOp::Gt => doc! { path: { "$gt": value.clone() } },
        FilterOp::Gte => doc! { path: { "$gte": value.clone() } },
        FilterOp::Lt => doc! { path: { "$lt": value.clone() } },
        FilterOp::Lte => doc! { path: { "$lte": value.clone() } },
        FilterOp::InSet => doc! { path: { "$in": operand_array(op, value)? } },
        FilterOp::NotInSet => doc! { path: { "$nin": operand_array(op, value)? } },
        FilterOp::Like => {
            let text = vantage_vista::operand_text(&crate::vista::cbor::bson_to_cbor(value));
            doc! { path: { "$regex": regex_escape(&text), "$options": "i" } }
        }
        // An operator added after this driver was written; the caller
        // evaluates it locally.
        _ => return unsupported(Capability::FilterOps),
    };
    Ok(filter)
}

/// Case-insensitive literal substring match of `text` on any of `paths`.
/// No paths matches nothing.
pub(crate) fn search_filter(paths: &[String], text: &str) -> Document {
    let escaped = regex_escape(text);
    let mut clauses: Vec<Document> = paths
        .iter()
        .map(|path| doc! { path: { "$regex": &escaped, "$options": "i" } })
        .collect();
    match clauses.len() {
        0 => doc! { "_id": { "$exists": false } },
        1 => clauses.remove(0),
        _ => doc! { "$or": clauses },
    }
}

/// A driver-native condition: a `bson::Document` or a `MongoCondition`. Any
/// other type is `IncorrectUsage`.
pub(crate) fn native_condition(condition: NativeCondition) -> Result<MongoCondition> {
    let condition = match condition.downcast::<Document>() {
        Ok(doc) => return Ok(MongoCondition::Doc(doc)),
        Err(other) => other,
    };
    condition.downcast::<MongoCondition>().map_err(|other| {
        error!(
            "MongoDB takes a bson::Document or MongoCondition as a native condition",
            key = other.key()
        )
        .mark_incorrect_usage()
    })
}

/// The key a filter document is recorded under in `Vista::query()`: its
/// canonical extended JSON, so equal documents (same fields, same order)
/// get equal keys.
#[cfg(feature = "rhai")]
pub(crate) fn native_key(filter: &Document) -> String {
    Bson::Document(filter.clone())
        .into_canonical_extjson()
        .to_string()
}

fn operand_array(op: FilterOp, value: &Bson) -> Result<Vec<Bson>> {
    match value {
        Bson::Array(items) => Ok(expand_all(items)),
        _ => Err(error!(
            "set operator needs an array operand",
            operator = format!("{op:?}")
        )),
    }
}
