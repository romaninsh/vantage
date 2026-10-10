//! Rhai vocabulary for MongoDB.
//!
//! MongoDB's query language is data, so there is no expression builder: a
//! script writes filters and pipeline stages as Rhai maps, which convert
//! straight to BSON (see [`convert`]). The vocabulary only adds constructors
//! for the BSON types a map literal cannot spell:
//!
//! - `oid("<24 hex chars>")` → `ObjectId` (`oid(<ObjectId>)` returns it as is)
//! - `date("<RFC 3339>")` → BSON `DateTime` (`date(<DateTime>)` likewise)
//! - `doc([[key, value], …])` → a document keeping the given key order
//!
//! `MongoTableShell::register_rhai_extensions` layers this under the vista
//! verbs and adds `self.with_condition(#{…})`.

pub mod convert;

use bson::oid::ObjectId;
use bson::{DateTime, Document};
use vantage_core::error;
use vantage_rhai::Vocab;
use vantage_rhai::rhai::{Array, Engine, EvalAltResult, ImmutableString};

pub use convert::{bson_to_dynamic, dynamic_to_bson, dynamic_to_document, map_to_document};

type FnResult<T> = std::result::Result<T, Box<EvalAltResult>>;

/// The MongoDB vocabulary as a [`vantage_rhai::Host`] takes it.
pub struct MongoVocab;

impl Vocab for MongoVocab {
    fn register(&self, engine: &mut Engine) {
        register_mongo_onto(engine);
    }
}

/// Register the BSON constructors and their types onto `engine`.
pub fn register_mongo_onto(engine: &mut Engine) {
    engine
        .register_type_with_name::<ObjectId>("ObjectId")
        .register_fn("to_string", |oid: &mut ObjectId| oid.to_hex())
        .register_fn("to_debug", |oid: &mut ObjectId| format!("oid(\"{oid}\")"))
        .register_fn("oid", parse_oid)
        .register_fn("oid", |oid: ObjectId| oid);

    engine
        .register_type_with_name::<DateTime>("DateTime")
        .register_fn("to_string", |dt: &mut DateTime| rfc3339(dt))
        .register_fn("to_debug", |dt: &mut DateTime| {
            format!("date(\"{}\")", rfc3339(dt))
        })
        .register_fn("date", parse_date)
        .register_fn("date", |dt: DateTime| dt);

    engine
        .register_type_with_name::<Document>("Document")
        .register_fn("to_string", |doc: &mut Document| doc.to_string())
        .register_fn("to_debug", |doc: &mut Document| doc.to_string())
        .register_fn("doc", ordered_doc);
}

fn parse_oid(hex: ImmutableString) -> FnResult<ObjectId> {
    ObjectId::parse_str(hex.as_str()).map_err(|e| {
        error!(
            "oid() needs a 24-character hex string",
            value = hex.to_string(),
            details = e.to_string()
        )
        .to_string()
        .into()
    })
}

fn parse_date(text: ImmutableString) -> FnResult<DateTime> {
    DateTime::parse_rfc3339_str(text.as_str()).map_err(|e| {
        error!(
            "date() needs an RFC 3339 timestamp",
            value = text.to_string(),
            details = e.to_string()
        )
        .to_string()
        .into()
    })
}

fn rfc3339(dt: &DateTime) -> String {
    dt.try_to_rfc3339_string()
        .unwrap_or_else(|_| dt.to_string())
}

/// `doc([["b", 1], ["a", 2]])`: each element a `[key, value]` pair.
fn ordered_doc(pairs: Array) -> FnResult<Document> {
    let mut doc = Document::new();
    for (index, pair) in pairs.into_iter().enumerate() {
        let pair = pair.try_cast::<Array>().filter(|p| p.len() == 2);
        let Some(mut pair) = pair else {
            return Err(error!("doc() needs [key, value] pairs", pair = index)
                .to_string()
                .into());
        };
        let value = pair.pop().expect("pair has two elements");
        let key = pair.pop().expect("pair has two elements");
        let Some(key) = key.into_immutable_string().ok() else {
            return Err(error!("doc() keys must be strings", pair = index)
                .to_string()
                .into());
        };
        if doc.contains_key(key.as_str()) {
            return Err(error!("doc() repeats a key", key = key.to_string())
                .to_string()
                .into());
        }
        let value = dynamic_to_bson(value).map_err(|e| e.to_string())?;
        doc.insert(key.to_string(), value);
    }
    Ok(doc)
}

#[cfg(test)]
mod tests {
    use super::*;
    use bson::{Bson, doc};
    use vantage_rhai::rhai::Dynamic;
    use vantage_rhai::{Block, Env, Host, Limits};

    fn eval(code: &str) -> std::result::Result<Dynamic, String> {
        let host = Host::builder(Limits::background())
            .vocab(MongoVocab)
            .build();
        host.compile(&Block::from(code))
            .and_then(|script| script.eval(&Env::new()))
            .map_err(|e| e.to_string())
    }

    fn bson(code: &str) -> Bson {
        dynamic_to_bson(eval(code).unwrap()).unwrap()
    }

    #[test]
    fn oid_builds_an_object_id() {
        let oid = ObjectId::new();
        assert_eq!(bson(&format!(r#"oid("{oid}")"#)), Bson::ObjectId(oid));
        assert_eq!(bson(&format!(r#"oid(oid("{oid}"))"#)), Bson::ObjectId(oid));
        let shown = eval(&format!(r#"`${{oid("{oid}")}}`"#)).unwrap();
        assert_eq!(shown.into_string().unwrap(), oid.to_hex());
    }

    #[test]
    fn bad_oid_errors() {
        let err = eval(r#"oid("nope")"#).unwrap_err();
        assert!(err.contains("oid()") && err.contains("nope"), "{err}");
    }

    #[test]
    fn date_builds_a_bson_datetime() {
        let expected = DateTime::parse_rfc3339_str("2026-01-02T03:04:05Z").unwrap();
        assert_eq!(
            bson(r#"date("2026-01-02T03:04:05Z")"#),
            Bson::DateTime(expected)
        );
        let err = eval(r#"date("yesterday")"#).unwrap_err();
        assert!(err.contains("date()") && err.contains("yesterday"), "{err}");
    }

    #[test]
    fn doc_keeps_key_order() {
        let built = bson(r#"doc([["b", 1], ["a", #{x: oid("000000000000000000000001")}]])"#);
        let Bson::Document(built) = built else {
            panic!("expected a document")
        };
        assert_eq!(built.keys().collect::<Vec<_>>(), vec!["b", "a"]);
        assert_eq!(
            built,
            doc! { "b": 1_i64, "a": { "x": ObjectId::parse_str("000000000000000000000001").unwrap() } }
        );
        let err = eval(r#"doc([["a"]])"#).unwrap_err();
        assert!(err.contains("pair"), "{err}");
    }

    #[test]
    fn doc_rejects_duplicate_keys() {
        let err = eval(r#"doc([["n", 1], ["dup", 2], ["dup", 3]])"#).unwrap_err();
        assert!(
            err.contains("repeats a key") && err.contains("dup"),
            "{err}"
        );
    }

    #[test]
    fn map_literal_converts_to_a_filter() {
        let filter = bson(r#"#{price: #{"$gt": 100}, tags: ["a", "b"], gone: ()}"#);
        assert_eq!(
            filter,
            Bson::Document(doc! { "gone": null, "price": { "$gt": 100_i64 }, "tags": ["a", "b"] })
        );
    }
}
