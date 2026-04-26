//! Read path — condition resolution, candidate-id seeding via index lookup,
//! and in-memory verification of remaining conditions.
//!
//! Algorithm (single condition is the common case):
//! 1. Resolve any deferred conditions concurrently.
//! 2. With no conditions → full scan.
//! 3. Otherwise the first condition seeds the candidate id list via
//!    `candidates_for`; for each candidate we fetch the row and verify the
//!    remaining conditions in memory with `condition_matches`.
//!
//! Conditions on the id column short-circuit to a direct `main.get(id)`.
//! Conditions on a non-indexed column **panic** at this layer — the
//! contract is that flagging columns `Indexed` is a deliberate, declarative
//! step.

use indexmap::IndexMap;
use redb::ReadableTable;
use std::collections::HashSet;

use vantage_core::{Result, error};
use vantage_table::table::Table;
use vantage_types::{Entity, Record};

use crate::condition::RedbCondition;
use crate::redb::helpers::{id_column_name, indexed_columns, paginate};
use crate::redb::{Redb, index_table_def, index_table_name, main_table_def};
use crate::types::{AnyRedbType, decode_record, value_to_index_key};

pub(crate) async fn load_filtered<E>(
    db: &Redb,
    table: &Table<Redb, E>,
) -> Result<IndexMap<String, Record<AnyRedbType>>>
where
    E: Entity<AnyRedbType>,
{
    let table_name = table.table_name();
    let id_col = id_column_name(table);
    let indexed = indexed_columns(table);

    // Resolve any deferred conditions up front (they may need DB roundtrips).
    let conditions: Vec<RedbCondition> =
        futures_util::future::try_join_all(table.conditions().map(|c| c.clone().resolve())).await?;

    let mut out: IndexMap<String, Record<AnyRedbType>> = IndexMap::new();

    if conditions.is_empty() {
        let txn = db.begin_read()?;
        let main = match txn.open_table(main_table_def(table_name)) {
            Ok(t) => t,
            // A wholesale truncate (`delete_all_values` with no conditions)
            // drops the main table; subsequent reads should see an empty set
            // rather than an error.
            Err(redb::TableError::TableDoesNotExist(_)) => return Ok(out),
            Err(e) => {
                return Err(error!(
                    "Failed to open table for scan",
                    details = e.to_string()
                ));
            }
        };
        let iter = main
            .iter()
            .map_err(|e| error!("Failed to iterate redb table", details = e.to_string()))?;
        for entry in iter {
            let (k, v) =
                entry.map_err(|e| error!("Failed to read redb row", details = e.to_string()))?;
            out.insert(k.value().to_string(), decode_record(v.value())?);
        }
        return Ok(paginate(table, out));
    }

    let (first, rest) = conditions.split_first().unwrap();
    let candidates = candidates_for(db, table_name, &id_col, &indexed, first)?;

    let txn = db.begin_read()?;
    let main = match txn.open_table(main_table_def(table_name)) {
        Ok(t) => t,
        // Same case as the unconditional scan: post-truncate reads should
        // see an empty set rather than error.
        Err(redb::TableError::TableDoesNotExist(_)) => return Ok(out),
        Err(e) => {
            return Err(error!(
                "Failed to open table for verify",
                details = e.to_string()
            ));
        }
    };

    for id in candidates {
        let row_bytes = match main
            .get(id.as_str())
            .map_err(|e| error!("Failed to read row by id", details = e.to_string()))?
        {
            Some(b) => b,
            None => continue, // dangling index entry — ignore
        };
        let record = decode_record(row_bytes.value())?;
        if rest
            .iter()
            .all(|c| condition_matches(c, &record, &id_col, &id))
        {
            out.insert(id, record);
        }
    }

    Ok(paginate(table, out))
}

/// Seed candidate IDs from a single condition. Panics if the condition
/// targets a column that's neither flagged `Indexed` nor the table id.
fn candidates_for(
    db: &Redb,
    table_name: &str,
    id_col: &str,
    indexed: &HashSet<String>,
    cond: &RedbCondition,
) -> Result<Vec<String>> {
    match cond {
        RedbCondition::Eq { column, value } => {
            if column == id_col {
                return id_lookup(db, table_name, std::slice::from_ref(value));
            }
            require_indexed(indexed, column);
            scan_index(db, table_name, column, value)
        }
        RedbCondition::In { column, values } => {
            if column == id_col {
                return id_lookup(db, table_name, values);
            }
            require_indexed(indexed, column);
            let mut out = Vec::new();
            let mut seen: HashSet<String> = HashSet::new();
            for v in values {
                for id in scan_index(db, table_name, column, v)? {
                    if seen.insert(id.clone()) {
                        out.push(id);
                    }
                }
            }
            Ok(out)
        }
        RedbCondition::Deferred(_) => {
            // Resolved in load_filtered before reaching here.
            unreachable!("deferred condition reached candidates_for after resolve()")
        }
    }
}

fn require_indexed(indexed: &HashSet<String>, column: &str) {
    if !indexed.contains(column) {
        panic!(
            "vantage-redb: condition on non-indexed column `{}` — flag the column with ColumnFlag::Indexed",
            column
        );
    }
}

/// Direct main-table lookup for one or more id values; preserves order
/// and skips ids that don't exist.
fn id_lookup(db: &Redb, table_name: &str, ids: &[AnyRedbType]) -> Result<Vec<String>> {
    let txn = db.begin_read()?;
    let main = match txn.open_table(main_table_def(table_name)) {
        Ok(t) => t,
        Err(redb::TableError::TableDoesNotExist(_)) => return Ok(Vec::new()),
        Err(e) => {
            return Err(error!(
                "Failed to open main for id lookup",
                details = e.to_string()
            ));
        }
    };
    let mut out = Vec::with_capacity(ids.len());
    for v in ids {
        let id_str = match v.try_get::<String>() {
            Some(s) => s,
            None => continue,
        };
        if main
            .get(id_str.as_str())
            .map_err(|e| error!("Main get failed", details = e.to_string()))?
            .is_some()
        {
            out.push(id_str);
        }
    }
    Ok(out)
}

/// Walk the index table for a single value, returning all matching IDs.
/// Composite key `(value_bytes, id) → ()` is range-scanned over the window
/// `[(value_bytes, ""), (value_bytes, sentinel)]`.
fn scan_index(
    db: &Redb,
    table_name: &str,
    column: &str,
    value: &AnyRedbType,
) -> Result<Vec<String>> {
    let key_bytes = value_to_index_key(value)?;
    let idx_name = index_table_name(table_name, column);
    let txn = db.begin_read()?;
    let idx = match txn.open_table(index_table_def(&idx_name)) {
        Ok(t) => t,
        Err(redb::TableError::TableDoesNotExist(_)) => return Ok(Vec::new()),
        Err(e) => {
            return Err(error!(
                "Failed to open index table",
                table = idx_name,
                details = e.to_string()
            ));
        }
    };

    let lo = (key_bytes.as_slice(), "");
    // Sentinel "id past any string" — no realistic id exceeds this.
    let hi_id = "\u{10FFFF}".repeat(2);
    let hi = (key_bytes.as_slice(), hi_id.as_str());

    let iter = idx
        .range(lo..=hi)
        .map_err(|e| error!("Index range scan failed", details = e.to_string()))?;

    let mut ids = Vec::new();
    for entry in iter {
        let (k, _) =
            entry.map_err(|e| error!("Index iteration failed", details = e.to_string()))?;
        let (vbytes, id) = k.value();
        if vbytes != key_bytes.as_slice() {
            break;
        }
        ids.push(id.to_string());
    }
    Ok(ids)
}

/// In-memory check for a condition against an already-loaded record.
fn condition_matches(
    cond: &RedbCondition,
    record: &Record<AnyRedbType>,
    id_col: &str,
    id: &str,
) -> bool {
    match cond {
        RedbCondition::Eq { column, value } => {
            if column == id_col {
                value.try_get::<String>().as_deref() == Some(id)
            } else {
                record
                    .get(column.as_str())
                    .map(|v| v.value() == value.value())
                    .unwrap_or(false)
            }
        }
        RedbCondition::In { column, values } => {
            if column == id_col {
                values
                    .iter()
                    .any(|v| v.try_get::<String>().as_deref() == Some(id))
            } else if let Some(field) = record.get(column.as_str()) {
                values.iter().any(|v| v.value() == field.value())
            } else {
                false
            }
        }
        RedbCondition::Deferred(_) => false,
    }
}
