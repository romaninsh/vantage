//! An `IdStrategy::Auto` create whose cache seed fails after the master
//! insert succeeded: the servo still binds the new id, so the next flash
//! patches that row instead of inserting a second one.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use async_trait::async_trait;
use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_core::{Result, error};
use vantage_diorama::{CacheBackend, CacheTable, FlashKind, IdStrategy, Lens, MemoryCacheTable};
use vantage_types::Record;
use vantage_vista::{Column, Vista, VistaMetadata, mocks::MockShell};

type Rec = Record<CborValue>;

/// A memory cache table whose inserts fail while `fail` is set.
struct FlakyTable {
    inner: MemoryCacheTable,
    fail: Arc<AtomicBool>,
}

#[async_trait]
impl CacheTable for FlakyTable {
    async fn list_values(&self) -> Result<IndexMap<String, Rec>> {
        self.inner.list_values().await
    }
    async fn get_value(&self, id: &str) -> Result<Option<Rec>> {
        self.inner.get_value(id).await
    }
    async fn insert_value(&self, id: &str, record: &Rec) -> Result<()> {
        if self.fail.load(Ordering::SeqCst) {
            return Err(error!("cache write failed", id = id));
        }
        self.inner.insert_value(id, record).await
    }
    async fn insert_values(&self, rows: IndexMap<String, Rec>) -> Result<()> {
        self.inner.insert_values(rows).await
    }
    async fn delete_value(&self, id: &str) -> Result<()> {
        self.inner.delete_value(id).await
    }
    async fn clear(&self) -> Result<()> {
        self.inner.clear().await
    }
    async fn count(&self) -> Result<i64> {
        self.inner.count().await
    }
}

struct FlakyCache {
    table: Arc<FlakyTable>,
}

#[async_trait]
impl CacheBackend for FlakyCache {
    async fn open_table(&self, _name: &str) -> Result<Arc<dyn CacheTable>> {
        Ok(self.table.clone() as Arc<dyn CacheTable>)
    }
}

fn product_vista(shell: &MockShell) -> Vista {
    let metadata = VistaMetadata::new()
        .with_column(Column::new("id", "String").with_flag("id"))
        .with_column(Column::new("name", "String"))
        .with_id_column("id");
    Vista::new("products", Box::new(shell.clone().with_metadata(metadata)))
}

#[tokio::test]
async fn a_failed_cache_seed_still_binds_the_created_id() -> Result<()> {
    let fail = Arc::new(AtomicBool::new(false));
    let cache = FlakyCache {
        table: Arc::new(FlakyTable {
            inner: MemoryCacheTable::default(),
            fail: fail.clone(),
        }),
    };
    let lens = Arc::new(
        Lens::new()
            .cache_source(Arc::new(cache))
            .build()
            .expect("build lens"),
    );
    let shell = MockShell::new();
    let dio = lens.make_dio(product_vista(&shell)).await?;
    let servo = dio.servo_new(IdStrategy::Auto);

    fail.store(true, Ordering::SeqCst);
    servo.set("name", CborValue::Text("Croissant".into()));
    servo.flash().await?.expect("returning insert fires");
    let id = servo.id().expect("bound despite the failed seed");
    assert_eq!(shell.record_ids(), vec![id.clone()]);

    fail.store(false, Ordering::SeqCst);
    servo.set("name", CborValue::Text("Pain".into()));
    let second = servo.flash().await?.expect("follow-up fires");
    assert_eq!(second.kind(), &FlashKind::Patch);
    assert_eq!(second.id(), Some(id.as_str()));
    assert_eq!(shell.record_ids(), vec![id], "no second insert");
    Ok(())
}
