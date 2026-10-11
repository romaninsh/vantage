//! Advertised write capabilities must not be refused.

use indexmap::IndexMap;
use vantage_dataset::traits::WritableValueSet;
use vantage_types::Record;

use super::Probe;
use crate::{CborValue, Vista};

/// Id used by the write probes. No fixture is expected to hold it.
const PROBE_ID: &str = "vista-contract-probe";

/// For each advertised write capability, call its method with an id the
/// fixture doesn't hold, under the lenient rule (any error but a refusal).
/// A probe may succeed and store a row, so run this only on a disposable
/// fixture.
pub async fn check_advertised_writes(vista: &Vista) {
    let caps = vista.capabilities().clone();
    let mut probe = Probe::default();
    let empty = Record::<CborValue>::new();

    if caps.can_insert {
        let inserted = vista.insert_value(PROBE_ID, &empty).await;
        probe.check("can_insert", "insert_value", &inserted);
        if inserted.is_ok() {
            let _ = vista.delete(PROBE_ID).await;
        }
    }
    if caps.can_update {
        probe.check(
            "can_update",
            "patch_value",
            &vista.patch_value(PROBE_ID, &empty).await,
        );
    }
    if caps.can_delete {
        probe.check("can_delete", "delete", &vista.delete(PROBE_ID).await);
    }
    if caps.can_import {
        probe.check(
            "can_import",
            "import_values",
            &vista.import_values(&IndexMap::new()).await,
        );
    }
    probe.finish();
}
