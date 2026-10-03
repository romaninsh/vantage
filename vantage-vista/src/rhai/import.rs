//! The `import_from(source)` / `import_from(source, mapping)` write terminal.
//!
//! `source` is another table handle, read through its narrowing. A mapping
//! builds each imported record from a source row. String values may reference
//! source columns as `${row.<col>}`:
//!
//! - a value that is exactly one reference (`"${row.tag}"`) copies the
//!   column's raw value, type included;
//! - any other string interpolates references into text (`"tag:${row.tag}"`);
//! - non-string values pass through as literals.
//!
//! The mapping must set the target's id column; the id commands the insert and
//! is not repeated in the record.
//!
//! The verb returns a report, `#{ inserted, skipped, cancelled }`: rows newly
//! written, rows the target already held, and whether the backend stopped
//! early (a host's import may be cancellable).

use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_dataset::WritableValueSet;
use vantage_rhai::rhai::{ImmutableString, Map as RhaiMap};
use vantage_rhai::template::{Part, split};
use vantage_types::{Record, cbor_id_to_string};

use super::bridge::block_on;
use super::convert::dynamic_to_cbor;
use super::handle::Handle;
use super::read::{RhaiResult, fetch_capped, rhai_err, run};
use super::vocab::TargetResolver;
use crate::vista::Vista;

type Rows = IndexMap<String, Record<CborValue>>;

/// Copy every row of `source` into `vista` as it is. Returns the report.
pub(crate) fn import_rows(
    vista: &Vista,
    source: &Handle,
    resolver: Option<&TargetResolver>,
) -> RhaiResult<RhaiMap> {
    write_records(vista, &source_rows(source, resolver)?)
}

/// Build one record per row of `source` through `mapping`, then write them.
pub(crate) fn import_rows_mapped(
    vista: &Vista,
    source: &Handle,
    mapping: &RhaiMap,
    resolver: Option<&TargetResolver>,
) -> RhaiResult<RhaiMap> {
    let rows = source_rows(source, resolver)?;
    let id_column = vista.get_id_column().unwrap_or("id");
    let records = mapped_records(&rows, mapping, id_column, vista.name())?;
    write_records(vista, &records)
}

fn source_rows(source: &Handle, resolver: Option<&TargetResolver>) -> RhaiResult<Rows> {
    let vista = source.resolve(resolver).map_err(rhai_err)?;
    let rows = run(fetch_capped(&vista, source.row_limit()))?;
    Ok(rows.into_iter().collect())
}

/// Bulk import when the backend can, one insert per row otherwise.
///
/// A bulk import reports how many rows were new; the rest count as skipped.
/// A backend whose import was cancelled part-way returns an error carrying
/// [`IMPORT_CANCELLED`]; that becomes a `cancelled` report, not a throw.
fn write_records(vista: &Vista, records: &Rows) -> RhaiResult<RhaiMap> {
    if vista.capabilities().can_import {
        return match block_on(vista.import_values(records)).map_err(rhai_err)? {
            Ok(inserted) => Ok(report(
                inserted,
                records.len().saturating_sub(inserted),
                false,
            )),
            Err(e) => match e.context.get(IMPORT_CANCELLED) {
                Some(n) => {
                    let skipped = e.context.get(IMPORT_CANCELLED_SKIPPED);
                    Ok(report(
                        n.parse().unwrap_or(0),
                        skipped.and_then(|s| s.parse().ok()).unwrap_or(0),
                        true,
                    ))
                }
                None => Err(rhai_err(e)),
            },
        };
    }
    for (id, record) in records {
        run(vista.insert_value(id.clone(), record))?;
    }
    Ok(report(records.len(), 0, false))
}

/// Context key a backend sets on the error it returns from
/// `import_vista_values` when the import was cancelled part-way; its value
/// is the number of rows inserted before the stop.
pub const IMPORT_CANCELLED: &str = "import_cancelled_after";

/// Context key set next to [`IMPORT_CANCELLED`]: the number of rows skipped
/// (already held) before the stop. Missing means none.
pub const IMPORT_CANCELLED_SKIPPED: &str = "import_cancelled_skipped";

fn report(inserted: usize, skipped: usize, cancelled: bool) -> RhaiMap {
    let mut map = RhaiMap::new();
    map.insert("inserted".into(), (inserted as i64).into());
    map.insert("skipped".into(), (skipped as i64).into());
    map.insert("cancelled".into(), cancelled.into());
    map
}

/// Evaluate `mapping` over every source row.
pub(crate) fn mapped_records(
    rows: &Rows,
    mapping: &RhaiMap,
    id_column: &str,
    table: &str,
) -> Result<Rows, String> {
    let mut records = Rows::with_capacity(rows.len());
    for (row_index, (row_id, row)) in rows.iter().enumerate() {
        let at = || format!("source row {} ({row_id})", row_index + 1);
        let mut fields: IndexMap<String, CborValue> = IndexMap::new();
        for (field, value) in mapping.iter() {
            let mapped = match value.read_lock::<ImmutableString>() {
                Some(template) => apply_template(template.as_str(), row)
                    .map_err(|e| format!("mapping `{field}` on {}: {e}", at()))?,
                None => {
                    dynamic_to_cbor(value.clone()).map_err(|e| format!("mapping `{field}`: {e}"))?
                }
            };
            fields.insert(field.to_string(), mapped);
        }
        let id = fields
            .shift_remove(id_column)
            .as_ref()
            .and_then(cbor_id_to_string)
            .ok_or_else(|| {
                format!(
                    "{} produced no `{id_column}`; the mapping must set the id column",
                    at()
                )
            })?;
        check_id_table(&id, table).map_err(|e| format!("{}: {e}", at()))?;
        if records
            .insert(id.clone(), Record::from_indexmap(fields))
            .is_some()
        {
            return Err(format!(
                "duplicate id `{id}` at {}; refusing to import a set that overwrites itself",
                at()
            ));
        }
    }
    Ok(records)
}

/// Refuse an id whose `table:` prefix names another table. Drivers that read
/// `table:key` out of an id would otherwise let a `:` in source data decide
/// where the record lands.
fn check_id_table(id: &str, table: &str) -> Result<(), String> {
    match id.split_once(':') {
        Some((named, _)) if named != table => Err(format!(
            "the mapped id `{id}` names table `{named}`, but this import writes to `{table}`"
        )),
        _ => Ok(()),
    }
}

/// `${row.<col>}` template over one row. A lone reference yields the value
/// with its type; anything else interpolates into text.
fn apply_template(template: &str, row: &Record<CborValue>) -> Result<CborValue, String> {
    if !template.contains("${") {
        return Ok(CborValue::Text(template.to_string()));
    }
    let parts = split(template).map_err(|_| format!("unterminated `${{` in `{template}`"))?;
    let column = |hole: &str| hole.trim().strip_prefix("row.").map(str::to_string);
    if let [Part::Hole(hole)] = parts.as_slice()
        && let Some(col) = column(hole)
    {
        return lookup(row, &col).cloned();
    }
    let mut out = String::new();
    for part in parts {
        match part {
            Part::Lit(text) => out.push_str(&text),
            Part::Hole(hole) => match column(&hole) {
                Some(col) => out.push_str(&stringify(lookup(row, &col)?)?),
                None => {
                    out.push_str("${");
                    out.push_str(&hole);
                    out.push('}');
                }
            },
        }
    }
    Ok(CborValue::Text(out))
}

fn lookup<'a>(row: &'a Record<CborValue>, column: &str) -> Result<&'a CborValue, String> {
    row.get(column)
        .ok_or_else(|| format!("source has no column `{column}`"))
}

fn stringify(value: &CborValue) -> Result<String, String> {
    match value {
        CborValue::Text(s) => Ok(s.clone()),
        CborValue::Integer(i) => Ok(i128::from(*i).to_string()),
        CborValue::Float(f) => Ok(f.to_string()),
        CborValue::Bool(b) => Ok(b.to_string()),
        other => Err(format!(
            "column value {other:?} does not interpolate into text"
        )),
    }
}
