//! Keyset (seek) pagination for `fetch_cursor`.
//!
//! A page is the next `size` documents strictly after the last row of the
//! previous page in the sort order: the table's orders followed by `_id`
//! ascending (unless `_id` is already a sort key), which makes the order
//! total. The continuation token carries the last row's sort-key values.
//!
//! A token belongs to the sort it was issued under. Changing the sort between
//! pages is undefined; only a key-count mismatch is detected.
//!
//! A missing field sorts as `null`, below every other value, and is carried as
//! `Bson::Null`. Comparison operators only match values of the same BSON type
//! class, while the sort orders across classes, so each "past" branch also
//! admits every class that sorts later (earlier when descending) via `$type`.
//! Array-valued sort fields are not supported: the server sorts them by their
//! min/max element, which the seek does not model.

use bson::{Bson, Document, doc};
use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_core::{Result, error};
use vantage_table::sorting::{OrderBy, SortDirection};
use vantage_table::table::Table;
use vantage_types::{EmptyEntity, Record};

use crate::condition::MongoCondition;
use crate::mongodb::MongoDB;
use crate::mongodb::impls::joins::imported_columns;
use crate::select::join::alias_index;
use crate::types::AnyMongoType;

use super::walk_bson_path;

/// `$type` aliases per BSON comparison class, in MongoDB's cross-type sort
/// order. Numeric types form one class.
const CLASS_TYPES: [&[&str]; 13] = [
    &["minKey"],
    &["null"],
    &["number"],
    &["string", "symbol"],
    &["object"],
    &["array"],
    &["binData"],
    &["objectId"],
    &["bool"],
    &["date"],
    &["timestamp"],
    &["regex"],
    &["maxKey"],
];

const MIN_KEY: usize = 0;
const NULL: usize = 1;
const NUMBER: usize = 2;
const MAX_KEY: usize = 12;

fn class_of(value: &Bson) -> Option<usize> {
    Some(match value {
        Bson::MinKey => MIN_KEY,
        Bson::Null | Bson::Undefined => NULL,
        Bson::Int32(_) | Bson::Int64(_) | Bson::Double(_) | Bson::Decimal128(_) => NUMBER,
        Bson::String(_) | Bson::Symbol(_) => 3,
        Bson::Document(_) => 4,
        Bson::Array(_) => 5,
        Bson::Binary(_) => 6,
        Bson::ObjectId(_) => 7,
        Bson::Boolean(_) => 8,
        Bson::DateTime(_) => 9,
        Bson::Timestamp(_) => 10,
        Bson::RegularExpression(_) => 11,
        Bson::MaxKey => MAX_KEY,
        _ => return None,
    })
}

/// Sort keys of the table's orders. The table needs its `_id` tiebreak (see
/// `add_id_tiebreak`) for the result to be a total order.
pub(crate) fn effective_sort(table: &Table<MongoDB, EmptyEntity>) -> Vec<(String, i32)> {
    table
        .orders()
        .filter_map(|(cond, direction)| match cond {
            MongoCondition::Doc(d) => d.keys().next().map(|key| {
                let dir = match direction {
                    SortDirection::Ascending => 1,
                    SortDirection::Descending => -1,
                };
                (key.clone(), dir)
            }),
            _ => None,
        })
        .collect()
}

/// Append `_id` ascending to the table's orders unless `_id` is already one.
pub(crate) fn add_id_tiebreak(table: &mut Table<MongoDB, EmptyEntity>) {
    if !effective_sort(table).iter().any(|(key, _)| key == "_id") {
        table.add_order(OrderBy {
            expression: MongoCondition::Doc(doc! { "_id": 1 }),
            direction: SortDirection::Ascending,
        });
    }
}

/// The sort-key values of a raw record of `table`, in `sort` order. A joined
/// column sorts by its alias but sits in the record under its dotted name.
/// Absent fields are `Null`.
pub(crate) fn sort_values(
    table: &Table<MongoDB, EmptyEntity>,
    record: &Record<AnyMongoType>,
    sort: &[(String, i32)],
) -> Vec<Bson> {
    let raw: IndexMap<String, Bson> = record
        .iter()
        .map(|(k, v)| (k.clone(), v.value().clone()))
        .collect();
    let joined = imported_columns(table);
    sort.iter()
        .map(|(key, _)| {
            let path: Vec<String> = match alias_index(key).and_then(|i| joined.get(i)) {
                Some(name) => vec![name.to_string()],
                None => key.split('.').map(str::to_string).collect(),
            };
            walk_bson_path(&raw, &path).unwrap_or(Bson::Null)
        })
        .collect()
}

/// Filter matching the documents strictly after `last` in `sort` order.
///
/// One `$or` branch per way of being past: the earlier keys equal, this key
/// past. With no way of being past (a single descending key whose last value
/// was null, say) nothing follows, and the filter matches no document: the
/// server rejects an empty `$or`.
pub(crate) fn seek_filter(sort: &[(String, i32)], last: &[Bson]) -> Document {
    let mut branches: Vec<Bson> = Vec::new();
    for (i, ((key, dir), value)) in sort.iter().zip(last).enumerate() {
        let prefix: Document = sort[..i]
            .iter()
            .zip(last)
            .map(|((k, _), v)| (k.clone(), v.clone()))
            .collect();
        for past in past_conditions(key, *dir, value) {
            let mut branch = prefix.clone();
            branch.extend(past);
            branches.push(Bson::Document(branch));
        }
    }
    if branches.is_empty() {
        return doc! { "_id": { "$exists": false } };
    }
    doc! { "$or": branches }
}

/// `$type` condition admitting every alias of the given classes.
fn type_condition(key: &str, classes: impl Iterator<Item = usize>) -> Option<Document> {
    let aliases: Vec<&str> = classes
        .flat_map(|c| CLASS_TYPES[c].iter().copied())
        .collect();
    (!aliases.is_empty()).then(|| doc! { key: { "$type": aliases } })
}

/// Alternative conditions for `key` to sort after `value`.
fn past_conditions(key: &str, dir: i32, value: &Bson) -> Vec<Document> {
    let class = class_of(value);
    let mut out = Vec::new();
    // Min/max keys and null have no same-class values to compare against.
    let comparable = !matches!(class, Some(MIN_KEY | NULL | MAX_KEY));
    if dir >= 0 {
        if class == Some(NULL) {
            return vec![doc! { key: { "$ne": Bson::Null } }];
        }
        if comparable {
            out.push(doc! { key: { "$gt": value.clone() } });
        }
        if let Some(c) = class {
            out.extend(type_condition(key, c + 1..CLASS_TYPES.len()));
        }
    } else {
        if class == Some(NULL) {
            return out;
        }
        if comparable {
            out.push(doc! { key: { "$lt": value.clone() } });
        }
        if let Some(c) = class {
            out.extend(type_condition(key, NUMBER..c));
            // Null and missing sort below every other class.
            out.push(doc! { key: Bson::Null });
        }
    }
    out
}

/// One BSON document per value, so every type — `ObjectId` and datetimes
/// included — survives the trip through the token.
pub(crate) fn encode_token(values: &[Bson]) -> Result<CborValue> {
    let items = values
        .iter()
        .map(|v| {
            let mut bytes = Vec::new();
            doc! { "v": v.clone() }
                .to_writer(&mut bytes)
                .map_err(|_| error!("could not encode fetch_cursor token for mongodb driver"))?;
            Ok(CborValue::Bytes(bytes))
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(CborValue::Array(items))
}

pub(crate) fn decode_token(token: &CborValue) -> Result<Vec<Bson>> {
    let invalid = || error!("invalid fetch_cursor token for mongodb driver");
    let CborValue::Array(items) = token else {
        return Err(invalid());
    };
    items
        .iter()
        .map(|item| {
            let CborValue::Bytes(bytes) = item else {
                return Err(invalid());
            };
            let mut document =
                Document::from_reader(&mut bytes.as_slice()).map_err(|_| invalid())?;
            document.remove("v").ok_or_else(invalid)
        })
        .collect()
}
