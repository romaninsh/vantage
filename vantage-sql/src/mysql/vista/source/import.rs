//! All-or-nothing bulk import, inside one transaction.
//!
//! Each record is prepared as a single insert would be (hooks, imported
//! columns, set invariants), and the id the caller keyed it under overwrites
//! any id the record carries. Records whose id the set already holds are
//! skipped untouched; an id held by a row outside the set, or a new record
//! that does not belong to it, is a conflict. The rest go in with multi-row
//! `INSERT` statements.
//!
//! MySQL compares ids under the id column's collation, as a single insert's
//! `WHERE id = ?` lookup does: with a case-insensitive collation an incoming
//! `A` finds a stored `a` and is skipped. Two *new* ids equal under the
//! collation (`X` and `x`) collide on the primary key and fail the whole
//! import.
//!
//! After-insert hooks run after the commit, for newly inserted records only.
//! They receive the prepared row as written, id included, so server-side
//! defaults are not in it. If a hook fails the import is already committed:
//! the error is returned and the remaining hooks do not run.
//!
//! Any failure before the commit rolls back and leaves the table as it was. A
//! concurrent writer taking an id between the existence check and the `INSERT`
//! fails the whole import (duplicate key).

mod lookup;
mod rows;

use ciborium::Value as CborValue;
use indexmap::IndexMap;
use sqlx::{MySql, Transaction};
use vantage_core::{Result, VantageError, error};
use vantage_table::table::Table;
use vantage_types::{Entity, Record};

use super::MysqlTableShell;
use crate::mysql::MysqlDB;
use crate::mysql::table_source::id_value;
use crate::mysql::types::AnyMysqlType;
use crate::sql_exec::execute_affected_on;
use crate::sql_vista::native_record;
use crate::table_writes::{held_outside, not_in_set};

/// Most rows per statement; keeps each one well under `max_allowed_packet`.
const MAX_ROWS: usize = 500;

/// Parameters a statement may bind. MySQL's limit is 65,535; the margin
/// leaves room for the parameters of the table's own conditions.
const PARAM_BUDGET: usize = 65_535 - 1_024;

/// How many rows go in one statement that binds `params_per_row`
/// parameters for each row.
fn rows_per_chunk(params_per_row: usize) -> usize {
    (PARAM_BUDGET / params_per_row.max(1)).clamp(1, MAX_ROWS)
}

/// A record ready to store under `id`; `row` carries the id column too.
struct Prepared {
    id: String,
    row: Record<AnyMysqlType>,
}

type Tx = Transaction<'static, MySql>;

fn failed(table: &str, e: sqlx::Error) -> VantageError {
    error!(
        "MySQL import failed",
        table = table,
        details = e.to_string()
    )
}

/// Import `records` (id → record); returns how many were newly inserted.
pub(super) async fn import<E>(
    shell: &MysqlTableShell<E>,
    records: &IndexMap<String, Record<CborValue>>,
) -> Result<usize>
where
    E: Entity<AnyMysqlType>,
{
    if records.is_empty() {
        return Ok(0);
    }
    let table = &shell.table;
    let id_field = table.id_field_name();
    let mut prepared = Vec::with_capacity(records.len());
    for (id, record) in records {
        let mut row = table
            .prepare_insert(&native_record::<MysqlDB>(record))
            .await?;
        row.insert(id_field.clone(), id_value(id));
        prepared.push(Prepared {
            id: id.clone(),
            row,
        });
    }

    let name = table.table_name();
    let mut tx = table
        .data_source()
        .pool()
        .begin()
        .await
        .map_err(|e| failed(name, e))?;
    let written = match write_new(table, &mut tx, prepared).await {
        Ok(written) => written,
        Err(e) => {
            // The original error is the useful one; a failed rollback still
            // discards the transaction when the connection drops it.
            let _ = tx.rollback().await;
            return Err(e);
        }
    };
    tx.commit().await.map_err(|e| failed(name, e))?;

    for item in &written {
        table.finish_insert(&item.id, &item.row).await?;
    }
    Ok(written.len())
}

/// Insert the records the table does not hold yet; returns those.
async fn write_new<E>(
    table: &Table<MysqlDB, E>,
    tx: &mut Tx,
    prepared: Vec<Prepared>,
) -> Result<Vec<Prepared>>
where
    E: Entity<AnyMysqlType>,
{
    let narrowed = table.conditions().next().is_some();
    let ids: Vec<&str> = prepared.iter().map(|p| p.id.as_str()).collect();
    let found = lookup::held(table, tx, &ids, narrowed).await?;
    if let Some(outside) = ids.iter().find(|id| found.get(**id) == Some(&false)) {
        return Err(held_outside(table, &outside.to_string()));
    }

    let new: Vec<Prepared> = prepared
        .into_iter()
        .filter(|p| !found.contains_key(&p.id))
        .collect();
    if new.is_empty() {
        return Ok(new);
    }

    if narrowed {
        let records: Vec<&Record<AnyMysqlType>> = new.iter().map(|p| &p.row).collect();
        let fitting = lookup::fitting(table, tx, &records).await?;
        if let Some(outside) = new.iter().find(|p| !fitting.contains(&p.id)) {
            return Err(not_in_set(table, &outside.id));
        }
    }

    let width = rows::columns(&new).len();
    for chunk in new.chunks(rows_per_chunk(width)) {
        execute_affected_on::<MysqlDB, _>(&mut **tx, &rows::insert(table.table_name(), chunk))
            .await?;
    }
    Ok(new)
}
