//! [`MockShell`]'s shared store, read and changed directly.
//!
//! These bypass the narrowing and the read-failure switch: a test reaching
//! into its own store, not a query. The store is shared by every clone, so a
//! handle kept before the shell went into a `Vista` changes what it reads.

use std::sync::atomic::Ordering;

use ciborium::Value as CborValue;

use super::{MockShell, Rec};

impl MockShell {
    /// While `true`, every read fails: a source that went offline.
    pub fn set_fail_reads(&self, fail: bool) {
        self.fail_reads.store(fail, Ordering::SeqCst);
    }

    /// Insert or replace a record by id.
    pub fn set_record(&self, id: impl Into<String>, record: Rec) {
        self.data.lock().unwrap().insert(id.into(), record);
    }

    /// Overwrite one field of an existing record; nothing when it is absent.
    pub fn set_field(&self, id: &str, field: &str, value: CborValue) {
        if let Some(rec) = self.data.lock().unwrap().get_mut(id) {
            rec.insert(field.to_string(), value);
        }
    }

    pub fn remove_record(&self, id: &str) {
        self.data.lock().unwrap().shift_remove(id);
    }

    pub fn clear_records(&self) {
        self.data.lock().unwrap().clear();
    }

    pub fn len(&self) -> usize {
        self.data.lock().unwrap().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn get_record(&self, id: &str) -> Option<Rec> {
        self.data.lock().unwrap().get(id).cloned()
    }

    /// Ids in store order.
    pub fn record_ids(&self) -> Vec<String> {
        self.data.lock().unwrap().keys().cloned().collect()
    }
}
