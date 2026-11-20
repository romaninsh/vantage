//! All-or-nothing bulk import, inside one multi-document transaction.
//!
//! Each record is prepared exactly as a single insert would (hooks, imported
//! columns, set invariants). Records already held by the set are skipped
//! untouched; an id held outside the set, or a new record that does not belong
//! to it, is a conflict. The rest are written with a single `insert_many`.
//!
//! After-insert hooks run once the transaction has committed, for the newly
//! inserted records only: an existing in-set id is skipped untouched, unlike a
//! single `insert_value` on an existing id, which returns the stored row and
//! fires its after hooks. If an after hook fails, the import is already
//! committed; the error is returned and the remaining hooks do not run.
//!
//! Any failure aborts the transaction, so the collection is left as it was.
//! The import is not retried on a `TransientTransactionError`: a concurrent
//! writer racing the same id fails the import as a whole. Transactions need a
//! replica set or sharded cluster, the same deployments that serve change
//! streams.
//!
//! Inside a block's transaction the import opens none of its own: it runs in
//! the block's session, lands when the block commits, and a failed write
//! fails the block.

use std::collections::HashSet;

use bson::{Bson, Document, doc};
use ciborium::Value as CborValue;
use futures_util::TryStreamExt;
use indexmap::IndexMap;
use mongodb::action::Find;
use mongodb::{ClientSession, Collection};
use vantage_core::{Result, VantageError, error};
use vantage_types::Record;

use super::MongoTableShell;
use crate::id::MongoId;
use crate::mongodb::tx::MongoTx;
use crate::mongodb::view::undeclared_view;
use crate::types::AnyMongoType;

/// A record ready to store: its id, the document, and the prepared record the
/// after-insert hooks see.
struct Prepared {
    id: MongoId,
    doc: Document,
    record: Record<AnyMongoType>,
}

fn failed(e: impl std::fmt::Display) -> VantageError {
    error!("MongoDB import failed", details = e.to_string())
}

/// Import `records` (id → record); returns how many were newly inserted.
pub(super) async fn import(
    shell: &MongoTableShell,
    records: &IndexMap<String, Record<CborValue>>,
) -> Result<usize> {
    shell
        .table
        .data_source()
        .ensure_writable(shell.table.table_name())?;
    if records.is_empty() {
        return Ok(0);
    }
    let prepared = prepare(shell, records).await?;

    // Inside a block's transaction the import is part of it: it runs in the
    // block's session and commits or rolls back with the block, which also
    // holds the after hooks until then.
    let written = match shell.table.data_source().tx() {
        Some(tx) => write_new(shell, &mut Session::Block(tx), prepared).await?,
        None => in_own_transaction(shell, prepared).await?,
    };

    for item in &written {
        shell.table.finish_insert(&item.id, &item.record).await?;
    }
    Ok(written.len())
}

async fn in_own_transaction(
    shell: &MongoTableShell,
    prepared: Vec<Prepared>,
) -> Result<Vec<Prepared>> {
    let client = shell.table.data_source().client();
    let mut session = client.start_session().await.map_err(failed)?;
    session.start_transaction().await.map_err(failed)?;

    let written = match write_new(shell, &mut Session::Own(&mut session), prepared).await {
        Ok(written) => written,
        Err(e) => {
            // The original error is the useful one; a failed abort leaves the
            // server to expire the transaction.
            let _ = session.abort_transaction().await;
            return Err(e);
        }
    };
    session.commit_transaction().await.map_err(failed)?;
    Ok(written)
}

async fn prepare(
    shell: &MongoTableShell,
    records: &IndexMap<String, Record<CborValue>>,
) -> Result<Vec<Prepared>> {
    let mut prepared = Vec::with_capacity(records.len());
    for (id, record) in records {
        let id = shell.parse_id(id);
        let record = shell
            .table
            .prepare_insert(&shell.flatten_for_write(record, None)?)
            .await?;
        let mut doc = doc! { "_id": id.to_bson() };
        for (key, value) in &record {
            doc.insert(key, value.value().clone());
        }
        prepared.push(Prepared { id, doc, record });
    }
    Ok(prepared)
}

/// Where the import's operations run: in its own session's transaction, or
/// in the block's. The block's session is locked for one operation at a
/// time, not the whole import, so the set's deferred conditions can read
/// through it in between.
enum Session<'a> {
    Own(&'a mut ClientSession),
    Block(&'a MongoTx),
}

async fn collect(
    find: Find<'_, Document>,
    session: &mut ClientSession,
) -> mongodb::error::Result<Vec<Document>> {
    let mut cursor = find.session(&mut *session).await?;
    cursor.stream(session).try_collect().await
}

/// The ids of the documents `filter` selects from the collection.
async fn held_ids(
    coll: &Collection<Document>,
    session: &mut Session<'_>,
    filter: Document,
) -> Result<HashSet<MongoId>> {
    let find = coll.find(filter).projection(doc! { "_id": 1 });
    let found = match session {
        Session::Own(session) => collect(find, session).await,
        Session::Block(tx) => {
            let mut session = tx.open().await?;
            tx.watch(collect(find, &mut session)).await
        }
    }
    .map_err(failed)?;
    Ok(found
        .iter()
        .filter_map(|d| d.get("_id").and_then(MongoId::from_bson))
        .collect())
}

/// Insert the records the collection does not hold yet; returns those.
async fn write_new(
    shell: &MongoTableShell,
    session: &mut Session<'_>,
    prepared: Vec<Prepared>,
) -> Result<Vec<Prepared>> {
    let source = shell.table.data_source();
    let coll = source.doc_collection(shell.table.table_name());
    let filter = source.set_filter(&shell.table).await?;

    let ids: Vec<Bson> = prepared.iter().map(|p| p.id.to_bson()).collect();
    let held = held_ids(&coll, session, doc! { "_id": { "$in": &ids } }).await?;
    if !held.is_empty() && !filter.is_empty() {
        let held_ids_bson: Vec<Bson> = held.iter().map(MongoId::to_bson).collect();
        let in_set = held_ids(
            &coll,
            session,
            doc! { "$and": [{ "_id": { "$in": held_ids_bson } }, &filter] },
        )
        .await?;
        if let Some(outside) = held.iter().find(|id| !in_set.contains(id)) {
            return Err(error!(
                "Id is held outside the table's set",
                id = outside.to_string()
            )
            .mark_conflict());
        }
    }

    let new: Vec<Prepared> = prepared
        .into_iter()
        .filter(|p| !held.contains(&p.id))
        .collect();
    if new.is_empty() {
        return Ok(new);
    }

    if !filter.is_empty() {
        let docs: Vec<Document> = new.iter().map(|p| p.doc.clone()).collect();
        let matching: HashSet<MongoId> = source
            .docs_matching(&filter, &docs)
            .await?
            .iter()
            .filter_map(|d| d.get("_id").and_then(MongoId::from_bson))
            .collect();
        if let Some(outside) = new.iter().find(|p| !matching.contains(&p.id)) {
            return Err(error!(
                "Document does not match the table's filter",
                id = outside.id.to_string()
            )
            .mark_conflict());
        }
    }

    let insert = coll.insert_many(new.iter().map(|p| &p.doc));
    match session {
        Session::Own(session) => insert.session(&mut **session).await,
        Session::Block(tx) => {
            let mut session = tx.open().await?;
            tx.watch(insert.session(&mut *session)).await
        }
    }
    .map_err(|e| undeclared_view(&e, shell.table.table_name()).unwrap_or_else(|| failed(e)))?;
    Ok(new)
}
