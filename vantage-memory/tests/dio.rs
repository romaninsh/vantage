use std::time::Duration;

use ciborium::Value as CborValue;
use vantage_dataset::prelude::ReadableValueSet;
use vantage_diorama::{Cache, Lens, Loader, LoaderCx};
use vantage_memory::{MemoryStore, MemoryVistaFactory};
use vantage_vista::{Capability, VistaFactory};

const T: &str =
    "name: t\nid_column: id\ncolumns:\n  id: { type: string, flags: [id] }\n  n: { type: int }\n";

async fn wait_for(dio: &vantage_diorama::Dio, id: &str, expect: Option<i64>) {
    for _ in 0..40 {
        let got = dio.cached().get_value(id).await.unwrap();
        let n = got.and_then(|r| match r.get("n") {
            Some(CborValue::Integer(i)) => Some(i128::from(*i) as i64),
            _ => None,
        });
        if n == expect {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("{id} never reached {expect:?}");
}

#[tokio::test]
async fn dio_over_memory_stays_live_through_watch_alone() {
    let store = MemoryStore::new();
    let t = store.table("t");
    t.upsert(
        "a",
        [("n".to_string(), CborValue::Integer(1.into()))]
            .into_iter()
            .collect(),
    );
    let master = MemoryVistaFactory::new(store.clone()).from_yaml(T).unwrap();
    let lens = Lens::builder()
        .cache(Cache::memory())
        .loader(Loader::eager(|cx: LoaderCx| async move {
            let rows = cx.master().list_values().await?;
            cx.reconcile(rows).await
        }))
        .build()
        .expect("lens");
    assert!(master.capabilities().has(Capability::Subscribe));
    let dio = lens.dio(master).live().build().await.unwrap();

    t.patch(
        "a",
        &[("n".to_string(), CborValue::Integer(2.into()))]
            .into_iter()
            .collect(),
    );
    wait_for(&dio, "a", Some(2)).await;
    t.upsert(
        "b",
        [("n".to_string(), CborValue::Integer(7.into()))]
            .into_iter()
            .collect(),
    );
    wait_for(&dio, "b", Some(7)).await;
    t.delete("a");
    wait_for(&dio, "a", None).await;
}
