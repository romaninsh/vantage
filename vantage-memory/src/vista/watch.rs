//! Live change stream for a `MemoryTableShell`: reduces the store table's
//! `MemoryChange` broadcast to `VistaChange`s scoped to one query snapshot,
//! taken when the watch starts.

use async_stream::try_stream;
use tokio::sync::broadcast::error::RecvError;
use vantage_core::Result;
use vantage_vista::{VistaChange, VistaChangeStream};

use crate::eval::matches_all;
use crate::{MemoryChange, MemoryTableHandle, Query};

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
