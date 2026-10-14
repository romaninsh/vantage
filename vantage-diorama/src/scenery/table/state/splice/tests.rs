use std::collections::HashSet;
use std::sync::Arc;

use ciborium::Value as CborValue;
use vantage_core::Result;
use vantage_types::Record;
use vantage_vista::mocks::MockShell;
use vantage_vista::{Column, SortDirection, Vista, VistaMetadata, VistaQuery};

use super::super::{Init, SharedRows};
use crate::scenery::table::view::{ViewHook, ViewId, ViewSignal};
use crate::{Cache, Dio, Lens};

fn order() -> VistaQuery {
    VistaQuery::new().with_order("amount", SortDirection::Ascending)
}

/// An eager Dio over an empty master; the tests fill its cache directly.
async fn dio() -> Dio {
    let metadata = VistaMetadata::new()
        .with_column(Column::new("id", "String").with_flag("id"))
        .with_column(Column::new("amount", "i64"))
        .with_id_column("id");
    let master = Vista::new("items", MockShell::new().with_metadata(metadata));
    let lens = Lens::builder()
        .cache(Cache::memory())
        .build()
        .expect("lens builds");
    lens.dio(master).build().await.expect("dio builds")
}

/// Rows ordered by amount, with no tasks running: nothing moves them but
/// the calls a test makes.
fn rows(dio: &Dio, signal: &Arc<ViewSignal>) -> Arc<SharedRows> {
    let init = Init {
        key: "items".to_string(),
        query: order(),
        base: Vec::new(),
        page_size: 50,
        titles_only: false,
        paged: false,
        read_cache: None,
        sugar_columns: Arc::new(HashSet::new()),
    };
    let view = (ViewId::next(), ViewHook::new(signal, None));
    SharedRows::new(&dio.inner, init, view).0
}

async fn write(dio: &Dio, amounts: &[(&str, i64)]) -> Result<()> {
    for (id, amount) in amounts {
        let mut rec = Record::new();
        rec.insert("id".to_string(), CborValue::Text(id.to_string()));
        rec.insert("amount".to_string(), CborValue::Integer((*amount).into()));
        dio.inner.cache.insert_value(id, &rec).await?;
    }
    Ok(())
}

/// The ids in slot order, checked against `id_to_idx`.
fn slots(rows: &SharedRows) -> Vec<String> {
    let map = rows.rows.read().unwrap();
    let id_to_idx = rows.id_to_idx.read().unwrap();
    let ids: Vec<String> = map
        .values()
        .filter_map(|row| match row.record.get("id") {
            Some(CborValue::Text(id)) => Some(id.clone()),
            _ => None,
        })
        .collect();
    for (slot, id) in ids.iter().enumerate() {
        assert_eq!(id_to_idx.get(id), Some(&slot), "{id} maps to its slot");
    }
    ids
}

/// The cache's ids as a full `sort_rows` orders them.
async fn sorted(dio: &Dio) -> Result<Vec<String>> {
    let mut all: Vec<_> = dio.inner.cache.list_values().await?.into_iter().collect();
    order().sort_rows(&mut all, &dio.inner.metadata);
    Ok(all.into_iter().map(|(id, _)| id).collect())
}

#[tokio::test]
async fn a_splice_after_a_reseed_places_by_the_new_order() -> Result<()> {
    let dio = dio().await;
    let signal = Arc::new(ViewSignal::new());
    let rows = rows(&dio, &signal);
    write(&dio, &[("a", 1), ("b", 2), ("c", 3)]).await?;
    rows.reseed_from_cache().await?;
    assert_eq!(slots(&rows), ["a", "b", "c"]);

    write(&dio, &[("c", 4)]).await?;
    assert!(rows.splice_one("c").await?);
    assert_eq!(slots(&rows), sorted(&dio).await?);
    assert!(rows.idx_to_id.lock().unwrap().is_some(), "the splice kept its slot index");

    // The same three rows in another order: `b` keeps slot 1, `a` and `c`
    // trade places.
    write(&dio, &[("a", 3), ("b", 2), ("c", 1)]).await?;
    rows.reseed_from_cache().await?;
    assert_eq!(slots(&rows), ["c", "b", "a"]);

    // `b` now ties `a`, and the id puts it after `a`. Searched with the
    // slots from before the reseed, the tie reads `c` and `b` stays put.
    write(&dio, &[("b", 3)]).await?;
    assert!(rows.splice_one("b").await?);
    assert_eq!(slots(&rows), sorted(&dio).await?);
    assert_eq!(slots(&rows), ["c", "a", "b"]);
    Ok(())
}
