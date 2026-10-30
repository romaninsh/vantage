//! The two set lookups an import makes inside its transaction, one statement
//! per chunk: which incoming ids the table holds (and whether in the set),
//! and which new rows would belong to the set.

use std::collections::{HashMap, HashSet};

use ciborium::Value as CborValue;
use vantage_core::{Result, error};
use vantage_expressions::{Expression, expr_any};
use vantage_table::table::Table;
use vantage_types::{Entity, Record};

use super::{Writer, rows_per_chunk};
use crate::mysql::MysqlDB;
use crate::mysql::table_source::id_value;
use crate::mysql::types::AnyMysqlType;
use crate::primitives::identifier::ident;
use crate::set_writes::{SetWrites, probe_columns, set_probe};

/// The incoming ids the table already holds, each mapped to whether its row
/// is in the set. MySQL compares each id with `id = ?` under the column's
/// collation, and the result carries the incoming id verbatim.
///
/// ```sql
/// SELECT `__import`.`v`,
///        EXISTS(SELECT 1 FROM t WHERE t.id = `__import`.`v`) AS held,
///        EXISTS(SELECT 1 FROM t WHERE t.id = `__import`.`v` AND <conditions>) AS in_set
/// FROM (SELECT ? AS v UNION ALL SELECT ? …) AS `__import`
/// ```
pub(super) async fn held<E>(
    table: &Table<MysqlDB, E>,
    tx: &mut Writer<'_>,
    ids: &[&str],
    narrowed: bool,
) -> Result<HashMap<String, bool>>
where
    E: Entity<AnyMysqlType>,
{
    let alias = ident("__import");
    let incoming: Expression<AnyMysqlType> = expr_any!("{}.{}", (alias), (ident("v")));
    let row_id: Expression<AnyMysqlType> = expr_any!(
        "{}.{}",
        (ident(table.table_name())),
        (ident(table.id_field_name()))
    );
    let mut matching = vec![expr_any!("{} = {}", (row_id), (incoming))];
    if narrowed {
        matching.extend(
            table
                .conditions()
                .map(|c| expr_any!("({})", (MysqlDB::condition_expr(c)))),
        );
    }
    let exists = |terms: &[Expression<AnyMysqlType>]| -> Expression<AnyMysqlType> {
        expr_any!(
            "EXISTS(SELECT 1 FROM {} WHERE {})",
            (ident(table.table_name())),
            (Expression::from_vec(terms.to_vec(), " AND "))
        )
    };
    let held_expr = exists(&matching[..1]);
    let in_set_expr = exists(&matching);

    let mut found = HashMap::new();
    for chunk in ids.chunks(rows_per_chunk(1)) {
        let values: Vec<Expression<AnyMysqlType>> = chunk
            .iter()
            .map(|id| {
                let value = id_value(id);
                expr_any!("SELECT {} AS {}", value, (ident("v")))
            })
            .collect();
        let query: Expression<AnyMysqlType> = expr_any!(
            "SELECT {} AS {}, {} AS {}, {} AS {} FROM ({}) AS {}",
            (incoming.clone()),
            (ident("v")),
            (held_expr.clone()),
            (ident("held")),
            (in_set_expr.clone()),
            (ident("in_set")),
            (Expression::from_vec(values, " UNION ALL ")),
            (alias.clone())
        );
        for row in result_rows(tx.fetch(&query).await?)? {
            if flag(row.get("held")) {
                found.insert(text(row.get("v"))?, flag(row.get("in_set")));
            }
        }
    }
    Ok(found)
}

/// The ids of the `records` (each carrying its id column) that satisfy the
/// table's conditions, checked over a derived table of the records as in
/// [`set_probe`]. Only for a table with conditions.
pub(super) async fn fitting<E>(
    table: &Table<MysqlDB, E>,
    tx: &mut Writer<'_>,
    records: &[&Record<AnyMysqlType>],
) -> Result<HashSet<String>>
where
    E: Entity<AnyMysqlType>,
{
    let id_field = table.id_field_name();
    let width = probe_columns(table, records).len();
    let mut fitting = HashSet::new();
    for chunk in records.chunks(rows_per_chunk(width)) {
        let select: Expression<AnyMysqlType> =
            expr_any!("{} AS {}", (ident(&id_field)), (ident("v")));
        let query = set_probe(table, chunk, select);
        for row in result_rows(tx.fetch(&query).await?)? {
            fitting.insert(text(row.get("v"))?);
        }
    }
    Ok(fitting)
}

/// The rows of a query result, each as column name → value.
fn result_rows(result: AnyMysqlType) -> Result<Vec<HashMap<String, CborValue>>> {
    let CborValue::Array(rows) = result.into_value() else {
        return Err(error!("expected array result"));
    };
    Ok(rows
        .into_iter()
        .filter_map(|row| match row {
            CborValue::Map(cells) => Some(
                cells
                    .into_iter()
                    .filter_map(|(k, v)| match k {
                        CborValue::Text(k) => Some((k, v)),
                        _ => None,
                    })
                    .collect(),
            ),
            _ => None,
        })
        .collect())
}

/// An `EXISTS(…)` cell: 1 or 0.
fn flag(cell: Option<&CborValue>) -> bool {
    match cell {
        Some(CborValue::Integer(i)) => i128::from(*i) != 0,
        Some(CborValue::Bool(b)) => *b,
        _ => false,
    }
}

/// An id echoed back from the statement's parameters.
fn text(cell: Option<&CborValue>) -> Result<String> {
    match cell {
        Some(CborValue::Text(s)) => Ok(s.clone()),
        Some(CborValue::Bytes(b)) => Ok(String::from_utf8_lossy(b).into_owned()),
        other => Err(error!(
            "import lookup returned no id",
            value = format!("{other:?}")
        )),
    }
}
