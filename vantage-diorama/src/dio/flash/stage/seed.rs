//! Row shapes a settle writes: an id-less insert's seeded row, a patch
//! merged over its base, and the id a stored record carries.

use ciborium::Value as CborValue;
use vantage_core::{Result, error};
use vantage_types::{Record, cbor_id_to_string};

use crate::dio::{Dio, DioEvent};
use crate::ops::{ChangeFlash, FlashOutcome};

/// An id-less insert the backend confirmed: the row joins the cache under
/// the id the write reported. Once the write landed the id is returned even
/// if seeding the cache fails (logged): the row exists upstream, and a
/// caller that lost its id would insert it again on retry.
pub(super) async fn seed_new_row(
    dio: &Dio,
    flash: &ChangeFlash,
    outcome: FlashOutcome,
) -> Result<FlashOutcome> {
    let inner = &dio.inner;
    let id = outcome
        .id
        .clone()
        .or_else(|| outcome.record.as_ref().and_then(|r| stored_id(dio, r)))
        .ok_or_else(|| {
            error!(
                "the write stored a new row without reporting its id",
                table = inner.cache_table_name
            )
        })?;
    let mut record = outcome.record.unwrap_or_else(|| flash.patch().clone());
    record
        .entry(dio.id_column().to_string())
        .or_insert_with(|| CborValue::Text(id.clone()));
    match inner.cache.insert_value(&id, &record).await {
        Ok(()) => {
            let _ = inner
                .event_bus
                .send(DioEvent::RecordInserted { id: id.clone() });
        }
        Err(e) => tracing::warn!(
            target: "vantage_diorama",
            id = %id,
            error = %e,
            "inserted row not seeded into the cache; it shows after the next refresh",
        ),
    }
    Ok(FlashOutcome {
        id: Some(id),
        record: Some(record),
    })
}

/// `flash`'s fields merged over `base`.
pub(super) fn merged(base: Option<&Record<CborValue>>, flash: &ChangeFlash) -> Record<CborValue> {
    let mut merged = base.cloned().unwrap_or_default();
    for (k, v) in flash.patch() {
        merged.insert(k.clone(), v.clone());
    }
    merged
}

/// The id a stored record carries in the Dio's id column, read in the form
/// every cache key takes.
pub(super) fn stored_id(dio: &Dio, record: &Record<CborValue>) -> Option<String> {
    record.get(dio.id_column()).and_then(cbor_id_to_string)
}
