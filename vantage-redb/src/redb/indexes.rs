//! Atomic index maintenance — both helpers run inside the caller's
//! `WriteTransaction` so index updates commit together with the main row.

use vantage_core::{Result, error};

use crate::redb::{index_table_def, index_table_name};
use crate::types::{AnyRedbType, value_to_index_key};

/// Insert `(value_bytes, id) → ()` into each indexed column's table.
pub(crate) fn write_indexes<'txn>(
    txn: &'txn redb::WriteTransaction,
    table_name: &str,
    indexed: &[(&'txn str, &'txn AnyRedbType)],
    id: &str,
) -> Result<()> {
    for (col, val) in indexed {
        let key_bytes = value_to_index_key(val)?;
        let idx_name = index_table_name(table_name, col);
        let mut idx = txn
            .open_table(index_table_def(&idx_name))
            .map_err(|e| error!("Failed to open index for write", details = e.to_string()))?;
        idx.insert((key_bytes.as_slice(), id), ())
            .map_err(|e| error!("Index insert failed", details = e.to_string()))?;
    }
    Ok(())
}

/// Remove `(value_bytes, id)` entries from each indexed column's table.
/// Missing index tables are silently skipped — that just means no entries
/// were ever inserted under that column.
pub(crate) fn delete_indexes<'txn>(
    txn: &'txn redb::WriteTransaction,
    table_name: &str,
    indexed: &[(&'txn str, &'txn AnyRedbType)],
    id: &str,
) -> Result<()> {
    for (col, val) in indexed {
        let key_bytes = value_to_index_key(val)?;
        let idx_name = index_table_name(table_name, col);
        let mut idx = match txn.open_table(index_table_def(&idx_name)) {
            Ok(t) => t,
            Err(redb::TableError::TableDoesNotExist(_)) => continue,
            Err(e) => {
                return Err(error!(
                    "Failed to open index for delete",
                    details = e.to_string()
                ));
            }
        };
        idx.remove((key_bytes.as_slice(), id))
            .map_err(|e| error!("Index delete failed", details = e.to_string()))?;
    }
    Ok(())
}
