//! The change feed: a SpacetimeDB subscription to the shell's set.
//!
//! # The conditions go into the subscription itself
//!
//! The query sent to the server is the same `SELECT * FROM t WHERE …` the read
//! path builds, so the server filters and only matching rows are ever sent.
//! SpacetimeDB maintains the subscription incrementally, so a row that *changes
//! out of* the filtered set arrives as a **delete**. Set membership is therefore
//! correct by construction: no re-read per notification and no whole-set
//! reload. A condition the dialect can't express is refused by
//! `add_condition`, so it can never be silently missing from the subscription.
//!
//! # Deletes and inserts become one `Updated`
//!
//! The wire format has no update: a changed row is a delete plus an insert of
//! the same key in one transaction. They are paired here by id, so a consumer
//! sees `Updated` and a grid repaints a row rather than removing and re-adding
//! it.

use std::collections::HashSet;

use ciborium::Value as CborValue;
use spacetimedb_sats::ProductType;
use vantage_core::{Result, error};
use vantage_types::Record;
use vantage_vista::{VistaChange, VistaChangeStream};

use super::SpacetimeTableShell;
use super::reads::{cbor_to_id, content_hash};
use crate::schema::RowIdentity;
use crate::value::row_to_record;

pub(super) async fn watch(shell: &SpacetimeTableShell) -> Result<VistaChangeStream> {
    let query = shell.select()?.render();
    let row_type = shell.row_type.clone();
    let columns = columns_in_order(&row_type);
    let identity = shell.identity.clone();
    let relation = shell.relation.clone();

    let mut subscription = shell
        .db
        .subscribe(query, row_type.clone(), relation.clone())
        .await?;

    let stream = async_stream::try_stream! {
        while let Some(delta) = subscription.recv().await {
            let delta = delta?;

            if delta.ephemeral {
                // Event-table rows exist only for the transaction that emitted
                // them, so pairing them as inserts would put rows in a cache that
                // the table itself will never list. `?` inside `try_stream!`
                // yields the error and ends the stream.
                //
                // A consumer reads a stream error as its cue to back off and
                // resubscribe, which meets the same refusal again. The factory
                // rejects event tables up front, so this is only reachable
                // against a v9 host, whose schema can't report the flag; there a
                // slow retry loop beats a grid filled with rows the table never
                // lists.
                Err(error!(
                    "SpacetimeDB sent event-table rows for this relation; their rows are \
                     deleted in the transaction that inserts them, so they cannot back a \
                     Vista",
                    table = relation.as_str()
                )
                .mark_unsupported()
                .traced())?;
            }

            let inserted = decode_all(&delta.inserts, &row_type, &columns, &identity)?;
            let deleted = decode_all(&delta.deletes, &row_type, &columns, &identity)?;

            // Pair by id within the transaction: an id on both sides means the
            // row changed rather than being replaced.
            let deleted_ids: HashSet<&String> = deleted.iter().map(|(id, _)| id).collect();
            let inserted_ids: HashSet<&String> = inserted.iter().map(|(id, _)| id).collect();

            for (id, record) in &inserted {
                if deleted_ids.contains(id) {
                    yield VistaChange::Updated { id: id.clone(), value: record.clone() };
                } else {
                    yield VistaChange::Inserted { id: id.clone(), value: record.clone() };
                }
            }
            for (id, _) in &deleted {
                // An unmatched delete left the set: removed outright, or changed
                // so it no longer satisfies the subscription's `WHERE`.
                if !inserted_ids.contains(id) {
                    yield VistaChange::Deleted { id: id.clone() };
                }
            }
        }
    };

    Ok(Box::pin(stream))
}

/// Column names in the row type's own order — the order a pushed BSATN row
/// decodes into.
fn columns_in_order(row_type: &ProductType) -> Vec<String> {
    row_type
        .elements
        .iter()
        .enumerate()
        .map(|(i, e)| {
            e.name()
                .map(|n| n.to_string())
                .unwrap_or_else(|| format!("column_{i}"))
        })
        .collect()
}

/// Decode a batch of pushed BSATN rows and pair each with its Vista id.
fn decode_all(
    rows: &[Vec<u8>],
    row_type: &ProductType,
    columns: &[String],
    identity: &RowIdentity,
) -> Result<Vec<(String, Record<CborValue>)>> {
    let mut out = Vec::with_capacity(rows.len());
    for bytes in rows {
        let value = crate::client::ws::decode_row(bytes, row_type)?;
        // A pushed row is always the whole row, so the table's own type is the
        // right one to render it against.
        let record = row_to_record(columns, row_type, &value);
        let id = match identity.column() {
            Some(column) => record.get(column).map(cbor_to_id).ok_or_else(|| {
                error!(
                    "a pushed row is missing its identity column",
                    column = column
                )
            })?,
            None => content_hash(&value),
        };
        out.push((id, record));
    }
    Ok(out)
}
