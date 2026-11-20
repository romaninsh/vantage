//! `vantage_vista::contract::check_temporal` over a memory table, and over the Dio facade
//! (`dio.cached()`) of a memory table. Both read and seed past the vista through the store.

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use ciborium::Value as CborValue;
use vantage_core::Result;
use vantage_dataset::traits::ReadableValueSet;
use vantage_diorama::{Cache, Dio, Lens, Loader, LoaderCx};
use vantage_memory::{MemoryStore, MemoryVistaFactory};
use vantage_types::{Record, temporal};
use vantage_vista::contract::{Foreign, Stored, TemporalFixture, check_temporal};
use vantage_vista::{Vista, VistaFactory};

const EVENT: &str = r#"
name: event
id_column: id
columns:
  id: { type: string, flags: [id, orderable] }
  d: { type: date, flags: [orderable] }
  t: { type: time, flags: [orderable] }
  ts: { type: timestamp, flags: [orderable] }
"#;

fn text(s: &str) -> CborValue {
    CborValue::Text(s.into())
}

fn tag(n: u64, s: &str) -> CborValue {
    CborValue::Tag(n, Box::new(text(s)))
}

async fn copy_whole_master(cx: LoaderCx) -> Result<()> {
    let rows = cx.master().list_values().await?;
    cx.reconcile(rows).await
}

/// A Dio over `master` whose in-memory cache is filled from the master on start.
async fn eager_dio(master: Vista) -> Dio {
    Lens::builder()
        .cache(Cache::memory())
        .loader(Loader::eager(copy_whole_master))
        .build()
        .expect("lens builds")
        .dio(master)
        .build()
        .await
        .expect("dio builds")
}

/// Keeps the last table's store (and Dio, when the vista is its facade), so `stored` and
/// `seed_foreign` reach past the vista.
struct Fixture {
    cached: bool,
    last: Mutex<Option<(MemoryStore, Option<Arc<Dio>>)>>,
}

impl Fixture {
    fn new(cached: bool) -> Self {
        Self {
            cached,
            last: Mutex::new(None),
        }
    }

    fn store(&self) -> MemoryStore {
        let last = self.last.lock().unwrap();
        last.as_ref().expect("fresh() ran first").0.clone()
    }
}

#[async_trait]
impl TemporalFixture for Fixture {
    async fn fresh(&self) -> Vista {
        let store = MemoryStore::new();
        let master = MemoryVistaFactory::new(store.clone())
            .from_yaml(EVENT)
            .expect("event spec builds");
        if !self.cached {
            *self.last.lock().unwrap() = Some((store, None));
            return master;
        }
        let dio = Arc::new(eager_dio(master).await);
        let vista = dio.cached();
        *self.last.lock().unwrap() = Some((store, Some(dio)));
        vista
    }

    async fn stored(&self, _vista: &Vista, id: &str, column: &str) -> Result<Stored> {
        let row = self.store().table("event").get(id);
        let value = row
            .and_then(|r| r.get(column).cloned())
            .unwrap_or(CborValue::Null);
        Ok(Stored::Carrier(value))
    }

    async fn seed_foreign(&self, _vista: &Vista) -> Result<Vec<Foreign>> {
        let day = tag(temporal::TAG_DATE, "2026-11-19");
        let at = tag(temporal::TAG_INSTANT, "2026-11-19T16:01:00.000000Z");
        let foreign = vec![
            (
                "d",
                tag(temporal::TAG_LEGACY_DATE, "2026-11-19"),
                day.clone(),
            ),
            ("d", text("2026-11-19"), day),
            (
                "t",
                text("16:01"),
                tag(temporal::TAG_TIME, "16:01:00.000000"),
            ),
            (
                "ts",
                tag(temporal::TAG_INSTANT, "2026-11-19 16:01:00"),
                at.clone(),
            ),
            (
                "ts",
                tag(temporal::TAG_INSTANT, "2026-11-19T17:01:00+01:00"),
                at,
            ),
        ];
        let table = self.store().table("event");
        let seeded = foreign
            .into_iter()
            .enumerate()
            .map(|(i, (column, written, reads_as))| {
                let row: Record<CborValue> = [(column.to_string(), written.clone())]
                    .into_iter()
                    .collect();
                table.upsert(&format!("f{i}"), row);
                Foreign::new(column, format!("{written:?}"), reads_as)
            })
            .collect();
        // Written behind the cache, so the Dio re-reads its master, as it would after any
        // outside change.
        let dio = self
            .last
            .lock()
            .unwrap()
            .as_ref()
            .and_then(|(_, dio)| dio.clone());
        if let Some(dio) = dio {
            dio.refresh().await?;
        }
        Ok(seeded)
    }
}

#[tokio::test]
async fn memory_temporal_contract() {
    check_temporal(&Fixture::new(false)).await;
}

#[tokio::test]
async fn diorama_temporal_contract() {
    check_temporal(&Fixture::new(true)).await;
}
