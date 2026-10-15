//! Rhai `Dynamic` ⇄ `Bson`.
//!
//! A Rhai map becomes a BSON document. Rhai keeps map keys sorted, so a
//! literal `#{b: 1, a: 2}` arrives as `{a: 2, b: 1}`; where key order matters
//! (a multi-key `$sort`), `mongo::doc([["b", 1], ["a", 2]])` builds a document
//! in the order given. `ObjectId`, BSON `DateTime` and `Document` values (from
//! `mongo::oid()`, `mongo::date()`, `mongo::doc()`) pass through as
//! themselves, as does any `Bson`
//! [`bson_to_dynamic`] could not map to a native Rhai value.

use bson::oid::ObjectId;
use bson::spec::BinarySubtype;
use bson::{Binary, Bson, DateTime, Document};
use vantage_core::{Result, error};
use vantage_rhai::rhai::{Array, Blob, Dynamic, Map};

/// Convert a script value to BSON. A value with no BSON form (a closure, a
/// table handle) is an error naming its type.
pub fn dynamic_to_bson(d: Dynamic) -> Result<Bson> {
    if d.is_unit() {
        return Ok(Bson::Null);
    }
    if d.is::<bool>() {
        return Ok(Bson::Boolean(d.cast::<bool>()));
    }
    if d.is::<i64>() {
        return Ok(Bson::Int64(d.cast::<i64>()));
    }
    if d.is::<f64>() {
        return Ok(Bson::Double(d.cast::<f64>()));
    }
    if d.is::<String>() {
        return Ok(Bson::String(d.cast::<String>()));
    }
    if d.is::<char>() {
        return Ok(Bson::String(d.cast::<char>().to_string()));
    }
    if d.is::<Blob>() {
        return Ok(Bson::Binary(Binary {
            subtype: BinarySubtype::Generic,
            bytes: d.cast::<Blob>(),
        }));
    }
    if d.is::<Array>() {
        return d
            .cast::<Array>()
            .into_iter()
            .map(dynamic_to_bson)
            .collect::<Result<Vec<_>>>()
            .map(Bson::Array);
    }
    if d.is::<Map>() {
        return map_to_document(d.cast::<Map>()).map(Bson::Document);
    }
    if d.is::<ObjectId>() {
        return Ok(Bson::ObjectId(d.cast::<ObjectId>()));
    }
    if d.is::<DateTime>() {
        return Ok(Bson::DateTime(d.cast::<DateTime>()));
    }
    if d.is::<Document>() {
        return Ok(Bson::Document(d.cast::<Document>()));
    }
    if d.is::<Bson>() {
        return Ok(d.cast::<Bson>());
    }
    Err(error!(
        "no BSON representation for rhai value",
        rhai_type = d.type_name()
    ))
}

/// A Rhai map as a BSON document, in the map's (sorted) key order.
pub fn map_to_document(map: Map) -> Result<Document> {
    let mut doc = Document::new();
    for (key, value) in map {
        let value = dynamic_to_bson(value).map_err(|e| {
            error!(
                "map value has no BSON representation",
                key = key.to_string(),
                details = e.to_string()
            )
        })?;
        doc.insert(key.to_string(), value);
    }
    Ok(doc)
}

/// A script value that must be a document: a map, or a `mongo::doc()` result.
pub fn dynamic_to_document(d: Dynamic) -> Result<Document> {
    let type_name = d.type_name();
    match dynamic_to_bson(d)? {
        Bson::Document(doc) => Ok(doc),
        _ => Err(error!("expected a map or document", rhai_type = type_name)),
    }
}

/// Convert BSON to a script value. Scalars, arrays and documents become
/// native Rhai values (a document a map); `ObjectId` and `DateTime` stay
/// typed so they round-trip into conditions unchanged. Anything else
/// (`Decimal128`, `Timestamp`, regexes, …) is carried as the `Bson` itself.
pub fn bson_to_dynamic(value: &Bson) -> Dynamic {
    match value {
        Bson::Null | Bson::Undefined => Dynamic::UNIT,
        Bson::Boolean(b) => Dynamic::from_bool(*b),
        Bson::Int32(i) => Dynamic::from_int(i64::from(*i)),
        Bson::Int64(i) => Dynamic::from_int(*i),
        Bson::Double(f) => Dynamic::from_float(*f),
        Bson::String(s) => Dynamic::from(s.clone()),
        Bson::Array(items) => Dynamic::from_array(items.iter().map(bson_to_dynamic).collect()),
        Bson::Document(doc) => Dynamic::from_map(
            doc.iter()
                .map(|(k, v)| (k.as_str().into(), bson_to_dynamic(v)))
                .collect(),
        ),
        Bson::ObjectId(oid) => Dynamic::from(*oid),
        Bson::DateTime(dt) => Dynamic::from(*dt),
        Bson::Binary(bin) if bin.subtype == BinarySubtype::Generic => {
            Dynamic::from_blob(bin.bytes.clone())
        }
        other => Dynamic::from(other.clone()),
    }
}
