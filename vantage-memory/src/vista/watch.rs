//! Live change stream for a `MemoryTableShell`: reduces the store table's
//! `MemoryChange` broadcast to `VistaChange`s scoped to one query snapshot,
//! taken when the watch starts.

use async_stream::{stream, try_stream};
use futures_util::stream::select_all;
use tokio::sync::broadcast::error::RecvError;
use vantage_core::Result;
use vantage_vista::{VistaChange, VistaChangeStream};

use crate::eval::matches_all;
use crate::{MemoryChange, MemoryTableHandle, Query};

/// `Invalidated` for every change to any of `tables`: a table with
/// roll-ups, then the tables they read. A changed row alone can't say which
/// rows' computed values moved, or whether they still match.
pub(crate) fn invalidations(tables: &[MemoryTableHandle]) -> VistaChangeStream {
    let streams = tables.iter().map(|table| {
        let mut rx = table.subscribe();
        let s: VistaChangeStream = Box::pin(stream! {
            while let Ok(_) | Err(RecvError::Lagged(_)) = rx.recv().await {
                yield Ok(VistaChange::Invalidated);
            }
        });
        s
    });
    Box::pin(select_all(streams.collect::<Vec<_>>()))
}

pub(crate) fn stream(table: MemoryTableHandle, query: Query) -> VistaChangeStream {
    let mut rx = table.subscribe();
    let s = try_stream! {
        loop {
            match rx.recv().await {
                Ok(change) => {
                    if let Some(vista_change) = to_vista_change(&query, change)? {
                        yield vista_change;
                    }
                }
                Err(RecvError::Lagged(_)) => yield VistaChange::Invalidated,
                Err(RecvError::Closed) => break,
            }
        }
    };
    Box::pin(s)
}

fn to_vista_change(query: &Query, change: MemoryChange) -> Result<Option<VistaChange>> {
    Ok(match change {
        MemoryChange::Inserted { id, row } => {
            matches_all(query, &row)?.then(|| VistaChange::Inserted {
                id,
                value: (*row).clone(),
            })
        }
        MemoryChange::Updated { id, row, old } => {
            match (matches_all(query, &old)?, matches_all(query, &row)?) {
                (false, true) => Some(VistaChange::Inserted {
                    id,
                    value: (*row).clone(),
                }),
                (true, true) => Some(VistaChange::Updated {
                    id,
                    value: (*row).clone(),
                }),
                (true, false) => Some(VistaChange::Deleted { id }),
                (false, false) => None,
            }
        }
        MemoryChange::Deleted { id, old } => {
            matches_all(query, &old)?.then_some(VistaChange::Deleted { id })
        }
        MemoryChange::Reset => Some(VistaChange::Invalidated),
    })
}
