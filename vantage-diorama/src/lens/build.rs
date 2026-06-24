use std::sync::Arc;

use ciborium::Value as CborValue;
use tokio::runtime::Handle;
use vantage_dataset::traits::ReadableValueSet;
use vantage_types::Record;
use vantage_vista_factory::VistaCatalog;

use crate::augment::Augmentation;
use crate::dio::Dio;
use crate::error::LensBuildError;
use crate::lens::CacheStatus;

use super::callbacks::{
    DioCallback, DioListPageCallback, DioLoadDetailCallback, boxed_dio_callback,
    boxed_list_page_callback, boxed_load_detail_callback,
};
use super::{Lens, LensBuilder, LensCallbacks};

impl LensBuilder {
    /// Validate required state and produce the built [`Lens`].
    ///
    /// When [`augment`](Self::augment) registered any augmentations and no
    /// explicit two-pass callbacks were supplied, this synthesizes them: a
    /// capability-aware list pass and an augmentation detail pass
    /// (`synth_list_page` / `synth_load_detail`). Registering the detail pass
    /// is what engages two-pass loading downstream.
    pub fn build(self) -> Result<Lens, LensBuildError> {
        if let Some(err) = self.deferred_cache_error {
            return Err(err);
        }
        let cache_source = self
            .cache_source
            .ok_or(LensBuildError::MissingCacheSource)?;
        let runtime = self
            .runtime
            .unwrap_or_else(|| Handle::try_current().expect("LensBuilder::build called outside a tokio runtime; supply one with .runtime(handle)"));

        let mut on_list_page = self.on_list_page;
        let mut on_load_detail = self.on_load_detail;
        let mut on_refresh = self.on_refresh;

        if !self.augmentations.is_empty() && on_load_detail.is_none() {
            let catalog = self.catalog.ok_or(LensBuildError::MissingCatalog)?;
            let augmentations = Arc::new(self.augmentations);

            // Each pass is synthesized only if the caller didn't supply one, so an
            // app can still hand-roll any single pass while reusing the rest.
            if on_list_page.is_none() {
                on_list_page = Some(synth_list_page());
            }
            if on_refresh.is_none() {
                on_refresh = Some(synth_refresh());
            }
            on_load_detail = Some(synth_load_detail(catalog, augmentations));
        }

        let callbacks = LensCallbacks {
            on_start: self.on_start,
            on_refresh,
            on_write: self.on_write,
            on_event: self.on_event,
            on_query: self.on_query,
            total_provider: self.total_provider,
            on_load_chunk: self.on_load_chunk,
            on_list_page,
            on_load_detail,
        };

        Ok(Lens {
            cache_source,
            callbacks: Arc::new(callbacks),
            defaults: self.defaults,
            runtime,
            activity: self.activity,
        })
    }
}

/// Capability-aware list pass for an augmented Dio.
///
/// When the master advertises `can_fetch_window` the requested
/// `[offset, offset+limit)` slice is pushed down to the driver; otherwise it
/// falls back to a full `list_values` windowed locally so sequential paging
/// still terminates. This is the generic form of what apps previously hand-rolled
/// — and it stops over-fetching the whole set on every page where the backend can
/// window.
fn synth_list_page() -> DioListPageCallback {
    boxed_list_page_callback(move |dio: &Dio, desc| {
        let dio = dio.clone();
        async move {
            let master = dio.master();
            if master.capabilities().can_fetch_window {
                master.fetch_window(desc.offset, desc.limit).await
            } else {
                use vantage_dataset::traits::ReadableValueSet;
                let rows = master.list_values().await?;
                Ok(rows
                    .into_iter()
                    .skip(desc.offset)
                    .take(desc.limit)
                    .collect())
            }
        }
    })
}

/// Augmentation detail pass: read the cheap list-pass row from the cache, run
/// every [`Augmentation`] against it (resolve a detail Vista, fetch, merge), and
/// return the enriched row.
fn synth_load_detail(
    catalog: Arc<VistaCatalog>,
    augmentations: Arc<Vec<Augmentation>>,
) -> DioLoadDetailCallback {
    boxed_load_detail_callback(move |dio: &Dio, id| {
        let dio = dio.clone();
        let catalog = catalog.clone();
        let augmentations = augmentations.clone();
        async move {
            let mut row = dio.cache().get_value(&id).await?.unwrap_or_default();
            let master_id_column = dio.master().get_id_column().unwrap_or("id").to_string();
            for aug in augmentations.iter() {
                aug.augment_row(&master_id_column, &mut row, &catalog)
                    .await?;
            }
            Ok(row)
        }
    })
}

/// Augmentation refresh: re-run the cheap **list pass** and reconcile it against
/// the cache without throwing away augmentation that is still valid.
///
/// Per id, only the keys the list pass paints are compared (the *non-augmented*
/// fields — the detail pass merges its columns on top). The verdict:
///
/// - **unchanged** list fields → leave the row as-is, keeping its hydrated detail
///   columns and `Complete` status (no re-augmentation).
/// - **changed** list fields → merge the fresh list values onto the cached record
///   and demote it to `Incomplete`, so the detail pass re-runs when it's next
///   visible.
/// - **new** id → stub `Incomplete`.
/// - **gone** (cached but no longer listed) → delete.
///
/// `dio.refresh()` (driven by an app's auto-refresh / change-probe) invokes this;
/// demoting to `Incomplete` is what restarts hydration on the next viewport pass.
fn synth_refresh() -> DioCallback {
    boxed_dio_callback(move |dio: &Dio| {
        let dio = dio.clone();
        async move {
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
    })
}

/// True when any field the list pass paints differs from the cached record. Only
/// keys present in `list_row` are compared — the detail pass's own columns (which
/// exist only in `cached`) are never inspected, so a row whose list fields are
/// byte-for-byte equal is left untouched.
fn list_fields_changed(list_row: &Record<CborValue>, cached: &Record<CborValue>) -> bool {
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
