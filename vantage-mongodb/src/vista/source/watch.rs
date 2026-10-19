//! Live updates for a vista, over a MongoDB change stream on its collection.
//!
//! The stream is opened without looking at document content: every write to
//! the collection arrives, and a narrowed vista's conditions are applied on
//! the re-read that turns an event into a row. A row that no longer matches
//! the vista is reported as `Deleted`, which the subscription contract asks
//! drivers (or consumers) to reconcile.
//!
//! Resuming is the driver's job. A `ChangeStream` that hits a resumable error
//! reopens itself once from its cached resume token, and re-arms that after
//! a successful reopen. An error that reaches this stream is therefore
//! either not resumable or a failed resume; it is yielded and the stream ends.

use bson::{Document, doc};
use futures_util::StreamExt;
use mongodb::change_stream::event::OperationType;
use vantage_core::{Result, error};
use vantage_dataset::traits::ReadableValueSet;
use vantage_vista::{VistaChange, VistaChangeStream};

use super::{MongoTableShell, project_record};
use crate::id::MongoId;
use crate::mongodb::view::undeclared_view;

/// Event kinds the stream is opened for. Anything else (index builds,
/// `createIndexes`, ...) is not a change to the rows.
const OPERATION_TYPES: [&str; 8] = [
    "insert",
    "update",
    "replace",
    "delete",
    "drop",
    "rename",
    "dropDatabase",
    "invalidate",
];

/// Open a change stream on the shell's collection. The cursor is established
/// server-side before this returns, so writes made afterwards are seen.
pub(super) async fn watch(shell: &MongoTableShell) -> Result<VistaChangeStream> {
    shell.table.check_read().await?;
    let table = shell.table.clone();
    let name = table.table_name();
    table.data_source().ensure_watchable(name)?;
    let column_paths = shell.column_paths.clone();
    let pipeline: Vec<Document> =
        vec![doc! { "$match": { "operationType": { "$in": OPERATION_TYPES.to_vec() } } }];

    // The stream runs on the data source's change-stream client; the re-read
    // behind each event (`get_value`) stays on the main one.
    let mut events = table
        .data_source()
        .stream_collection(name)
        .await?
        .watch()
        .pipeline(pipeline)
        .await
        .map_err(|e| {
            undeclared_view(&e, name).unwrap_or_else(|| {
                error!(
                    "Failed to open MongoDB change stream",
                    details = e.to_string()
                )
            })
        })?;

    Ok(Box::pin(async_stream::try_stream! {
        while let Some(event) = events.next().await {
            let event = event.map_err(|e| {
                error!("MongoDB change stream failed", details = e.to_string())
            })?;

            match event.operation_type {
                OperationType::Insert
                | OperationType::Update
                | OperationType::Replace
                | OperationType::Delete => {}
                // The collection or its database is gone or moved; the
                // stream has nothing left to say.
                OperationType::Drop
                | OperationType::Rename
                | OperationType::DropDatabase
                | OperationType::Invalidate => break,
                _ => continue,
            }

            let Some(id) = event
                .document_key
                .as_ref()
                .and_then(|key| key.get("_id"))
                .and_then(MongoId::from_bson)
            else {
                // A key this driver cannot name (neither an ObjectId nor a
                // string, which the list path rejects too); tell the consumer
                // to re-read.
                yield VistaChange::Invalidated;
                continue;
            };
            let key = id.to_string();

            if event.operation_type == OperationType::Delete {
                yield VistaChange::Deleted { id: key };
                continue;
            }

            let inserted = event.operation_type == OperationType::Insert;
            match table.get_value(id).await? {
                Some(record) => {
                    let value = project_record(&column_paths, record);
                    if inserted {
                        yield VistaChange::Inserted { id: key, value };
                    } else {
                        yield VistaChange::Updated { id: key, value };
                    }
                }
                // Deleted since, or outside the vista's conditions.
                None => yield VistaChange::Deleted { id: key },
            }
        }
    }))
}
