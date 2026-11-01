//! Advertised write capabilities must not be refused.

use indexmap::IndexMap;
use vantage_dataset::traits::WritableValueSet;
use vantage_types::Record;

use super::Probe;
use crate::{Capability, CborValue, FilterOp, Vista};

/// Id used by the write probes. No fixture is expected to hold it.
const PROBE_ID: &str = "vista-contract-probe";

/// For each advertised write capability, call every method of its family
/// with an id the fixture doesn't hold, under the lenient rule (any error but
/// a refusal):
///
/// - `Insert`: `insert_value`;
/// - `Update`: `patch_value`, `replace_value`, `upsert_value`;
/// - `Delete`: `delete`;
/// - `DeleteAll` (implied by `Delete` unless denied): `delete_all`;
/// - `Import`: `import_values` of no rows;
/// - `ConfineWrites`: `insert_value` and (with `DeleteAll`) `delete_all` through
///   a clone narrowed to the probe id. The un-narrowed `count()` must be the
///   same before the insert and after the `delete_all`: only the probe row
///   may go. The fixture must hold rows besides the probe row, or a
///   `delete_all` that ignores the narrowing leaves the count unchanged.
///
/// Without `ConfineWrites`, `delete_all` runs on `vista` itself, last, and
/// **empties the fixture**. Pass an un-narrowed, disposable vista: every
/// probe here may store or remove rows.
pub async fn check_advertised_writes(vista: &Vista) {
    let caps = *vista.capabilities();
    let mut probe = Probe::default();
    let empty = Record::<CborValue>::new();
    let cleanup = || async {
        if caps.has(Capability::Delete) {
            let _ = vista.delete(PROBE_ID).await;
        }
    };

    if caps.has(Capability::Insert) {
        let inserted = vista.insert_value(PROBE_ID, &empty).await;
        probe.check(Capability::Insert, "insert_value", &inserted);
        cleanup().await;
    }
    if caps.has(Capability::Update) {
        let patched = vista.patch_value(PROBE_ID, &empty).await;
        probe.check(Capability::Update, "patch_value", &patched);
        let replaced = vista.replace_value(PROBE_ID, &empty).await;
        probe.check(Capability::Update, "replace_value", &replaced);
        cleanup().await;
        let upserted = vista.upsert_value(PROBE_ID, &empty).await;
        probe.check(Capability::Update, "upsert_value", &upserted);
        cleanup().await;
    }
    if caps.has(Capability::Delete) {
        probe.check(Capability::Delete, "delete", &vista.delete(PROBE_ID).await);
    }
    if caps.has(Capability::Import) {
        probe.check(
            Capability::Import,
            "import_values",
            &vista.import_values(&IndexMap::new()).await,
        );
    }
    if caps.has(Capability::ConfineWrites) {
        probe_confined(vista, &mut probe).await;
    } else if caps.has(Capability::DeleteAll) {
        probe.check(
            Capability::DeleteAll,
            "delete_all",
            &vista.delete_all().await,
        );
    }
    probe.finish();
}

/// The `ConfineWrites` probes, through a clone narrowed to the probe id.
async fn probe_confined(vista: &Vista, probe: &mut Probe) {
    let caps = *vista.capabilities();
    let Some(id) = vista.id_column() else {
        return probe.fail(
            "`confine_writes` is advertised but the vista has no id column to narrow by"
                .to_string(),
        );
    };
    let confined = match vista.clone().with_condition(id, FilterOp::Eq, PROBE_ID) {
        Ok(confined) => confined,
        Err(e) => {
            return probe.check_ok(
                Capability::ConfineWrites,
                "with_condition(id = probe)",
                &Err::<(), _>(e),
            );
        }
    };
    let before = vista.count().await;
    if caps.has(Capability::Insert) {
        let inserted = confined.insert_value(PROBE_ID, &Record::new()).await;
        probe.check(
            Capability::ConfineWrites,
            "narrowed insert_value",
            &inserted,
        );
    }
    if caps.has(Capability::DeleteAll) {
        let deleted = confined.delete_all().await;
        probe.check(Capability::DeleteAll, "narrowed delete_all", &deleted);
        match (before, vista.count().await) {
            (Ok(before), Ok(after)) if before != after => probe.fail(format!(
                "`confine_writes`: a delete_all narrowed to the probe row changed the \
                 un-narrowed count from {before} to {after}"
            )),
            (Ok(_), Ok(_)) => {}
            (Err(e), _) | (_, Err(e)) => probe.check_ok(
                Capability::ConfineWrites,
                "count around the narrowed delete_all",
                &Err::<(), _>(e),
            ),
        }
    }
}
