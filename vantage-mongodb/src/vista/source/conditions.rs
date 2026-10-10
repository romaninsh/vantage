//! Filter documents for the vista source's `add_eq_condition` /
//! `add_op_condition`, and the downcast behind `add_raw_condition`.
//!
//! Operands that look like ids are matched in both spellings (`ObjectId` and
//! its 24-char hex string), because a text-typed surface cannot tell which one
//! the collection stores.

use std::any::Any;

use bson::{Bson, Document, doc};
use vantage_core::{Result, error};
use vantage_vista::FilterOp;

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

/// A boxed driver-native condition: a `bson::Document` or a `MongoCondition`.
pub(crate) fn raw_condition(condition: Box<dyn Any + Send + Sync>) -> Result<MongoCondition> {
    let condition = match condition.downcast::<Document>() {
        Ok(doc) => return Ok(MongoCondition::Doc(*doc)),
        Err(other) => other,
    };
    condition
        .downcast::<MongoCondition>()
        .map(|c| *c)
        .map_err(|_| {
            error!(
                "add_raw_condition expected a bson::Document or MongoCondition",
                source_type = std::any::type_name::<super::MongoTableShell>()
            )
        })
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

#[cfg(test)]
mod tests {
    use super::*;
    use bson::oid::ObjectId;

    fn s(v: &str) -> Bson {
        Bson::String(v.into())
    }

    #[test]
    fn comparison_operators_render() {
        let v = Bson::Int32(100);
        assert_eq!(
            op_filter("price", FilterOp::Eq, &v).unwrap(),
            doc! { "price": 100 }
        );
        assert_eq!(
            op_filter("price", FilterOp::Ne, &v).unwrap(),
            doc! { "price": { "$ne": 100 } }
        );
        assert_eq!(
            op_filter("price", FilterOp::Gt, &v).unwrap(),
            doc! { "price": { "$gt": 100 } }
        );
        assert_eq!(
            op_filter("price", FilterOp::Gte, &v).unwrap(),
            doc! { "price": { "$gte": 100 } }
        );
        assert_eq!(
            op_filter("price", FilterOp::Lt, &v).unwrap(),
            doc! { "price": { "$lt": 100 } }
        );
        assert_eq!(
            op_filter("price", FilterOp::Lte, &v).unwrap(),
            doc! { "price": { "$lte": 100 } }
        );
    }

    #[test]
    fn set_operators_render() {
        let v = Bson::Array(vec![s("a"), s("b")]);
        assert_eq!(
            op_filter("n", FilterOp::InSet, &v).unwrap(),
            doc! { "n": { "$in": ["a", "b"] } }
        );
        assert_eq!(
            op_filter("n", FilterOp::NotInSet, &v).unwrap(),
            doc! { "n": { "$nin": ["a", "b"] } }
        );
    }

    #[test]
    fn set_operator_rejects_non_array() {
        assert!(op_filter("n", FilterOp::InSet, &s("a")).is_err());
        assert!(op_filter("n", FilterOp::NotInSet, &Bson::Int32(1)).is_err());
    }

    #[test]
    fn like_escapes_and_ignores_case() {
        assert_eq!(
            op_filter("name", FilterOp::Like, &s("a.b")).unwrap(),
            doc! { "name": { "$regex": "a\\.b", "$options": "i" } }
        );
    }

    #[test]
    fn search_spans_paths_with_escaped_literal() {
        let paths = vec!["name".to_string(), "address.city".to_string()];
        assert_eq!(
            search_filter(&paths, "a.b"),
            doc! { "$or": [
                { "name": { "$regex": "a\\.b", "$options": "i" } },
                { "address.city": { "$regex": "a\\.b", "$options": "i" } },
            ] }
        );
    }

    #[test]
    fn search_without_paths_matches_nothing() {
        assert_eq!(
            search_filter(&[], "x"),
            doc! { "_id": { "$exists": false } }
        );
    }

    #[test]
    fn eq_on_hex_string_matches_both_spellings() {
        let oid = ObjectId::new();
        assert_eq!(
            op_filter("_id", FilterOp::Eq, &s(&oid.to_hex())).unwrap(),
            doc! { "_id": { "$in": [oid.to_hex(), oid] } }
        );
    }

    #[test]
    fn ne_on_object_id_excludes_both_spellings() {
        let oid = ObjectId::new();
        assert_eq!(
            op_filter("_id", FilterOp::Ne, &Bson::ObjectId(oid)).unwrap(),
            doc! { "_id": { "$nin": [oid, oid.to_hex()] } }
        );
    }

    #[test]
    fn in_set_extends_ids_with_counterparts() {
        let oid = ObjectId::new();
        let v = Bson::Array(vec![s(&oid.to_hex()), s("plain")]);
        assert_eq!(
            op_filter("_id", FilterOp::InSet, &v).unwrap(),
            doc! { "_id": { "$in": [oid.to_hex(), oid, "plain"] } }
        );
    }

    #[test]
    fn raw_condition_accepts_document_and_mongo_condition() {
        let from_doc = raw_condition(Box::new(doc! { "a": 1 })).unwrap();
        assert!(matches!(from_doc, MongoCondition::Doc(d) if d == doc! { "a": 1 }));
        let from_cond = raw_condition(Box::new(MongoCondition::Doc(doc! { "b": 2 }))).unwrap();
        assert!(matches!(from_cond, MongoCondition::Doc(d) if d == doc! { "b": 2 }));
    }

    #[test]
    fn raw_condition_rejects_other_types() {
        let err = raw_condition(Box::new(42_i32)).unwrap_err();
        assert!(err.to_string().contains("bson::Document or MongoCondition"));
    }

    #[test]
    fn ordering_operators_are_not_expanded() {
        let oid = ObjectId::new();
        assert_eq!(
            op_filter("_id", FilterOp::Gt, &s(&oid.to_hex())).unwrap(),
            doc! { "_id": { "$gt": oid.to_hex() } }
        );
    }
}
