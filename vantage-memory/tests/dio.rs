use std::sync::Arc;
use std::time::Duration;

use ciborium::Value as CborValue;
use vantage_dataset::prelude::ReadableValueSet;
use vantage_diorama::Lens;
use vantage_memory::{MemoryStore, MemoryVistaFactory};
use vantage_vista::VistaFactory;

const T: &str =
    "name: t\nid_column: id\ncolumns:\n  id: { type: string, flags: [id] }\n  n: { type: int }\n";

async fn wait_for(dio: &vantage_diorama::Dio, id: &str, expect: Option<i64>) {
    for _ in 0..40 {
        let got = dio.cache().get_value(id).await.unwrap();
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
    let lens = Arc::new(
        Lens::new()
            .cache_in_memory()
            .on_start(|dio| {
                let dio = dio.clone();
                async move {
                    let rows = dio.master().list_values().await?;
                    dio.cache().insert_values(rows).await?;
                    Ok(())
                }
            })
            .build()
            .expect("lens"),
    );
    let dio = lens.make_dio(master).await.unwrap();
    assert!(dio.master().can_watch());
    dio.watch().await.unwrap();

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
