//! `check_temporal` over `MockShell`, which stores values as given, and over `Canonical`, a
//! wrapper that converts to the carrier forms the way a driver must.

use std::sync::Mutex;

use async_trait::async_trait;
use ciborium::Value as CborValue;
use vantage_types::{Record, temporal};
use vantage_vista::contract::{Foreign, Stored, TemporalFixture, check_temporal};
use vantage_vista::{
    Column, ColumnFlag, Condition, TableShell, Vista, VistaMetadata, mocks::MockShell,
};

use super::{rec, t};

type Rec = Record<CborValue>;

/// Writes, reads and conditions in the carrier forms, by each column's declared type.
#[derive(Clone)]
struct Canonical {
    inner: Box<dyn TableShell>,
}

impl Canonical {
    fn record(&self, record: &Rec) -> Rec {
        record
            .iter()
            .map(|(k, v)| (k.clone(), self.value(k, v)))
            .collect()
    }

    fn value(&self, column: &str, value: &CborValue) -> CborValue {
        let kind = self
            .inner
            .metadata()
            .columns
            .get(column)
            .and_then(|c| c.original_type.clone())
            .unwrap_or_default();
        canonical(&kind, value)
    }
}

fn canonical(kind: &str, value: &CborValue) -> CborValue {
    if let CborValue::Array(items) = value {
        return CborValue::Array(items.iter().map(|v| canonical(kind, v)).collect());
    }
    match kind {
        "date" => temporal::as_date(value)
            .or_else(|| temporal::midnight_date(value))
            .map(|d| temporal::date(&d)),
        "time" => temporal::as_time(value).map(|t| temporal::time(&t)),
        "timestamp" => temporal::as_instant(value).map(|at| temporal::instant(&at)),
        _ => None,
    }
    .unwrap_or_else(|| value.clone())
}

vantage_vista::forward_table_shell!(Canonical, inner, {
    async fn list(&self) -> vantage_core::Result<indexmap::IndexMap<String, Rec>> {
        Ok(self
            .inner
            .list()
            .await?
            .into_iter()
            .map(|(id, r)| (id, self.record(&r)))
            .collect())
    }
    async fn get(&self, id: &str) -> vantage_core::Result<Option<Rec>> {
        Ok(self.inner.get(id).await?.map(|r| self.record(&r)))
    }
    fn add_condition(&mut self, condition: &Condition) -> vantage_core::Result<()> {
        let value = self.value(&condition.column, &condition.value);
        self.inner.add_condition(&Condition::new(
            condition.column.clone(),
            condition.op,
            value,
        ))
    }
    async fn insert(
        &self,
        id: Option<&str>,
        record: &Rec,
    ) -> vantage_core::Result<(String, vantage_dataset::Inserted<CborValue>)> {
        self.inner.insert(id, &self.record(record)).await
    }
    async fn replace(&self, id: &str, record: &Rec) -> vantage_core::Result<Rec> {
        self.inner.replace(id, &self.record(record)).await
    }
    async fn patch(&self, id: &str, partial: &Rec) -> vantage_core::Result<Rec> {
        self.inner.patch(id, &self.record(partial)).await
    }
});

fn shell() -> MockShell {
    let orderable =
        |name: &str, kind: &str| Column::new(name, kind).with_flag(ColumnFlag::Orderable);
    MockShell::new().with_metadata(
        VistaMetadata::new()
            .with_column(orderable("id", "String").with_flag(ColumnFlag::Id))
            .with_column(orderable("d", "date"))
            .with_column(orderable("t", "time"))
            .with_column(orderable("ts", "timestamp"))
            .with_id_column("id"),
    )
}

/// Keeps the last table's store, so `stored` and `seed_foreign` reach past the vista.
struct Fixture {
    canonical: bool,
    store: Mutex<Option<MockShell>>,
}

impl Fixture {
    fn new(canonical: bool) -> Self {
        Self {
            canonical,
            store: Mutex::new(None),
        }
    }

    fn store(&self) -> MockShell {
        self.store
            .lock()
            .unwrap()
            .clone()
            .expect("fresh() ran first")
    }
}

#[async_trait]
impl TemporalFixture for Fixture {
    async fn fresh(&self) -> Vista {
        let store = shell();
        *self.store.lock().unwrap() = Some(store.clone());
        let vista = Vista::new("event", store);
        if self.canonical {
            vista.map_shell(|inner| Canonical { inner })
        } else {
            vista
        }
    }

    async fn stored(&self, _vista: &Vista, id: &str, column: &str) -> vantage_core::Result<Stored> {
        let row = self.store().get(id).await?.unwrap_or_default();
        Ok(Stored::Carrier(
            row.get(column).cloned().unwrap_or(CborValue::Null),
        ))
    }

    async fn seed_foreign(&self, _vista: &Vista) -> vantage_core::Result<Vec<Foreign>> {
        let tag = |n: u64, s: &str| CborValue::Tag(n, Box::new(t(s)));
        let foreign = vec![
            (
                "d",
                tag(temporal::TAG_LEGACY_DATE, "2026-11-19"),
                tag(temporal::TAG_DATE, "2026-11-19"),
            ),
            ("d", t("2026-11-19"), tag(temporal::TAG_DATE, "2026-11-19")),
            ("t", t("16:01"), tag(temporal::TAG_TIME, "16:01:00.000000")),
            (
                "ts",
                tag(temporal::TAG_INSTANT, "2026-11-19 16:01:00"),
                tag(temporal::TAG_INSTANT, "2026-11-19T16:01:00.000000Z"),
            ),
            (
                "ts",
                tag(temporal::TAG_INSTANT, "2026-11-19T17:01:00+01:00"),
                tag(temporal::TAG_INSTANT, "2026-11-19T16:01:00.000000Z"),
            ),
        ];
        let store = self.store();
        Ok(foreign
            .into_iter()
            .enumerate()
            .map(|(i, (column, written, reads_as))| {
                let id = format!("f{i}");
                store.set_record(
                    id.clone(),
                    rec(&[("id", t(&id)), (column, written.clone())]),
                );
                Foreign::new(column, format!("{written:?}"), reads_as)
            })
            .collect())
    }
}

#[tokio::test]
async fn mock_temporal_contract() {
    check_temporal(&Fixture::new(true)).await;
}

/// The check reads values, not just whether a write was accepted: a store that keeps what it's
/// given fails.
#[tokio::test]
#[should_panic(expected = "accepted_inputs_normalise")]
async fn mock_temporal_catches_a_store_that_keeps_what_its_given() {
    check_temporal(&Fixture::new(false)).await;
}
