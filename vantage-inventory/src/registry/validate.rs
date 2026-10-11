//! The project-wide validation pass: snapshot every kind's keys and
//! indexes, then run each leaf's [`Kind::validate`](crate::Kind::validate)
//! against that one snapshot.

use std::collections::HashMap;

use super::Inventory;
use crate::keys::{IndexSink, KeySnapshot};

impl Inventory {
    /// The keys of every kind plus the indexes kinds contribute.
    pub fn snapshot(&self) -> KeySnapshot {
        let mut keys = HashMap::new();
        let mut sink = IndexSink::default();
        for (kind, leaves) in self.kinds.iter().zip(&self.leaves) {
            keys.insert(kind.id().to_string(), leaves.keys().cloned().collect());
            for (key, slot) in leaves {
                if let Some(value) = &slot.value {
                    kind.index_any(key, value, &mut sink);
                }
            }
        }
        KeySnapshot::new(keys, sink)
    }

    /// Re-run every kind's validation against the current keys, and say
    /// whether any leaf's errors changed. Leaves without a value, or
    /// holding reported failure messages, are skipped.
    pub(crate) fn revalidate(&mut self) -> bool {
        let snapshot = self.snapshot();
        let mut changed = false;
        for (kind, leaves) in self.kinds.iter().zip(&mut self.leaves) {
            for (key, slot) in leaves.iter_mut() {
                let Some(value) = slot.value.as_ref().filter(|_| !slot.reported) else {
                    continue;
                };
                let errors = kind.validate_any(key, value, &snapshot);
                if slot.errors != errors {
                    slot.errors = errors;
                    changed = true;
                }
            }
        }
        changed
    }
}
