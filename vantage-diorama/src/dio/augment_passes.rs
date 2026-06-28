//! The two-pass augmentation passes, owned by the Dio.
//!
//! A Dio configured with [`Dio::augment`](crate::Dio::augment) drives its own
//! list / detail / refresh passes from the augmentation config it holds, instead
//! of the Lens carrying them. These free functions are the shared bodies: the
//! Dio path calls them reading config off `DioInner`, and the legacy
//! `Lens::augment` synthesis (`lens::build`) delegates here too, so there's one
//! definition of each pass.

use ciborium::Value as CborValue;
use vantage_core::Result;
use vantage_dataset::traits::ReadableValueSet;
use vantage_types::Record;
use vantage_vista_factory::VistaCatalog;

use crate::augment::Augmentation;
use crate::dio::Dio;
use crate::lens::CacheStatus;
use crate::ops::QueryDescriptor;

/// Capability-aware **list pass**: push the `[offset, offset+limit)` window down
/// to the master when it advertises `can_fetch_window`, otherwise list the whole
/// set and window it locally so sequential paging still terminates.
pub(crate) async fn list_page(
    dio: &Dio,
    desc: QueryDescriptor,
) -> Result<Vec<(String, Record<CborValue>)>> {
    let master = dio.master();
    if master.capabilities().can_fetch_window {
        master.fetch_window(desc.offset, desc.limit).await
    } else {
        let rows = master.list_values().await?;
        Ok(rows
            .into_iter()
            .skip(desc.offset)
            .take(desc.limit)
            .collect())
    }
}

/// **Detail pass**: read the cheap list-pass row from the cache, run every
/// [`Augmentation`] against it (resolve a detail Vista, fetch, merge), return the
/// enriched row.
pub(crate) async fn load_detail_with(
    dio: &Dio,
    id: String,
    catalog: &VistaCatalog,
    augmentations: &[Augmentation],
) -> Result<Record<CborValue>> {
    let mut row = dio.cache().get_value(&id).await?.unwrap_or_default();
    let master_id_column = dio.master().get_id_column().unwrap_or("id").to_string();
    for aug in augmentations {
        aug.augment_row(&master_id_column, &mut row, catalog).await?;
    }
    Ok(row)
}

/// **Refresh pass**: re-run the cheap list pass and reconcile against the cache
/// without discarding still-valid augmentation. Unchanged list fields keep their
/// hydrated detail columns + `Complete` status; changed ones merge the fresh
/// list values and demote to `Incomplete` (re-hydrate on next viewport); new ids
/// stub `Incomplete`; vanished ids are deleted.
pub(crate) async fn refresh(dio: &Dio) -> Result<()> {
    let fresh = dio.master().list_values().await?;
    let cache = dio.cache();
    let existing = cache.list_values_with_status().await?;

    for (id, list_row) in &fresh {
        match existing.get(id) {
            Some((old, _)) => {
                if !list_fields_changed(list_row, old) {
                    continue;
                }
                let mut merged = old.clone();
                for (k, v) in list_row {
                    merged.insert(k.clone(), v.clone());
                }
                cache
                    .insert_value_with_status(id, &merged, CacheStatus::Incomplete)
                    .await?;
            }
            None => {
                cache
                    .insert_value_with_status(id, list_row, CacheStatus::Incomplete)
                    .await?;
            }
        }
    }
    for id in existing.keys() {
        if !fresh.contains_key(id) {
            cache.delete_value(id).await?;
        }
    }
    Ok(())
}

/// True when any field the list pass paints differs from the cached record. Only
/// keys present in `list_row` are compared — the detail pass's own columns (which
/// exist only in `cached`) are never inspected.
pub(crate) fn list_fields_changed(
    list_row: &Record<CborValue>,
    cached: &Record<CborValue>,
) -> bool {
    list_row.iter().any(|(k, v)| cached.get(k) != Some(v))
}

#[cfg(test)]
mod tests {
    use super::list_fields_changed;
    use ciborium::Value;
    use vantage_types::Record;

    fn rec(pairs: &[(&str, Value)]) -> Record<Value> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect()
    }

    #[test]
    fn unchanged_list_fields_ignore_augmented_columns() {
        let list = rec(&[
            ("status", Value::from("LIVE")),
            ("updated_at", Value::from("t1")),
        ]);
        let cached = rec(&[
            ("status", Value::from("LIVE")),
            ("updated_at", Value::from("t1")),
            ("subject_area", Value::from("finance")),
        ]);
        assert!(!list_fields_changed(&list, &cached));
    }

    #[test]
    fn a_changed_non_augmented_field_is_detected() {
        let list = rec(&[
            ("status", Value::from("DECOMMISSIONED")),
            ("updated_at", Value::from("t2")),
        ]);
        let cached = rec(&[
            ("status", Value::from("LIVE")),
            ("updated_at", Value::from("t1")),
            ("subject_area", Value::from("finance")),
        ]);
        assert!(list_fields_changed(&list, &cached));
    }

    #[test]
    fn a_new_list_key_absent_from_cache_counts_as_changed() {
        let list = rec(&[
            ("status", Value::from("LIVE")),
            ("region", Value::from("eu")),
        ]);
        let cached = rec(&[("status", Value::from("LIVE"))]);
        assert!(list_fields_changed(&list, &cached));
    }
}
