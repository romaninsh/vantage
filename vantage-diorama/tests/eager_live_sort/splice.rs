//! One live change on an eager, ordered view moves, inserts or drops that
//! one row in place: no whole-cache list, and the same order a fresh
//! `sort_rows` over the master gives, nulls and ties included.

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use ciborium::Value as CborValue;
use vantage_core::Result;
use vantage_dataset::prelude::ReadableValueSet;
use vantage_diorama::{Cache, ChangeEvent, Dio, Lens, TableScenery};
use vantage_types::Record;
use vantage_vista::mocks::MockShell;
use vantage_vista::{Column, SortDirection, Vista, VistaMetadata, VistaQuery};

use crate::support::spy_cache::{SpyCache, SpyTable};
use crate::support::{copy_master, eventually};

const SETTLE: Duration = Duration::from_secs(5);

fn metadata() -> VistaMetadata {
    VistaMetadata::new()
        .with_column(Column::new("id", "String").with_flag("id"))
        .with_column(Column::new("amount", "i64"))
        .with_id_column("id")
}

/// A row whose amount is `Some(value)`, or absent for `None`.
fn rec(id: &str, amount: Option<CborValue>) -> Record<CborValue> {
    let mut r = Record::new();
    r.insert("id".to_string(), CborValue::Text(id.to_string()));
    if let Some(amount) = amount {
        r.insert("amount".to_string(), amount);
    }
    r
}

fn int(n: i64) -> Option<CborValue> {
    Some(CborValue::Integer(n.into()))
}

struct Fixture {
    dio: Dio,
    master: MockShell,
    spy: Arc<SpyTable>,
}

async fn fixture(rows: impl IntoIterator<Item = (String, Record<CborValue>)>) -> Fixture {
    let master = rows.into_iter().fold(
        MockShell::new().with_metadata(metadata()),
        |shell, (id, r)| shell.with_record(id, r),
    );
    let spy = Arc::new(SpyTable::default());
    let lens = Lens::builder()
        .cache(Cache::backend(Arc::new(SpyCache(spy.clone()))))
        .loader(copy_master())
        .build()
        .expect("lens builds");
    let dio = lens
        .dio(Vista::new("items", master.clone()))
        .build()
        .await
        .expect("dio builds");
    Fixture { dio, master, spy }
}

impl Fixture {
    async fn view(&self, dir: SortDirection) -> Result<Arc<dyn TableScenery>> {
        let view = self.dio.table_scenery().order("amount", dir).open().await?;
        let len = self.master.len();
        assert!(
            eventually(SETTLE, || view.row_count() == len).await,
            "seeded"
        );
        Ok(view)
    }

    /// Write the change to the master, then push it as a streaming
    /// forwarder does.
    async fn push(&self, change: ChangeEvent) -> Result<()> {
        match &change {
            ChangeEvent::Updated { id, new: Some(r) }
            | ChangeEvent::Inserted { id, new: Some(r) } => {
                self.master.set_record(id.clone(), r.clone())
            }
            ChangeEvent::Deleted { id } => self.master.remove_record(id),
            _ => {}
        }
        self.dio.apply_change(change).await
    }

    /// The master's ids as a fresh `sort_rows` orders them.
    async fn sorted(&self, dir: SortDirection) -> Result<Vec<String>> {
        let mut rows: Vec<_> = self.dio.master().list_values().await?.into_iter().collect();
        VistaQuery::new()
            .with_order("amount", dir)
            .sort_rows(&mut rows, &metadata());
        Ok(rows.into_iter().map(|(id, _)| id).collect())
    }

    /// The whole-cache lists so far, once no open reseed is still listing.
    async fn lists(&self) -> usize {
        loop {
            let before = self.spy.lists.load(Ordering::SeqCst);
            tokio::time::sleep(Duration::from_millis(30)).await;
            let after = self.spy.lists.load(Ordering::SeqCst);
            if after == before {
                return after;
            }
        }
    }
}

fn ids(view: &Arc<dyn TableScenery>) -> Vec<String> {
    (0..view.row_count())
        .filter_map(|i| match view.row(i)?.record.get("id") {
            Some(CborValue::Text(t)) => Some(t.clone()),
            _ => None,
        })
        .collect()
}

async fn shows(view: &Arc<dyn TableScenery>, expected: &[String]) -> bool {
    eventually(SETTLE, || ids(view) == expected).await
}

fn five_hundred() -> impl Iterator<Item = (String, Record<CborValue>)> {
    (0..500).map(|n| {
        let id = format!("r{n}");
        let r = rec(&id, int(n % 97));
        (id, r)
    })
}

#[tokio::test]
async fn a_patch_moves_one_row_without_a_reseed() -> Result<()> {
    let f = fixture(five_hundred()).await;
    let view = f.view(SortDirection::Ascending).await?;
    let lists = f.lists().await;

    let new = Some(rec("r10", int(10_000)));
    f.push(ChangeEvent::Updated {
        id: "r10".into(),
        new,
    })
    .await?;
    let last = |v: &Arc<dyn TableScenery>| {
        v.row(499)
            .is_some_and(|r| r.record.get("id") == Some(&CborValue::Text("r10".into())))
    };
    assert!(eventually(SETTLE, || last(&view)).await, "r10 moved last");
    assert_eq!(f.lists().await, lists, "no whole-cache list");
    assert_eq!(ids(&view), f.sorted(SortDirection::Ascending).await?);
    Ok(())
}

#[tokio::test]
async fn an_insert_and_a_delete_splice_in_place() -> Result<()> {
    let f = fixture(five_hundred()).await;
    let view = f.view(SortDirection::Ascending).await?;
    let lists = f.lists().await;

    let new = Some(rec("new", int(40)));
    f.push(ChangeEvent::Inserted {
        id: "new".into(),
        new,
    })
    .await?;
    let expected = f.sorted(SortDirection::Ascending).await?;
    assert_eq!(expected.len(), 501);
    assert!(shows(&view, &expected).await, "inserted: {:?}", ids(&view));

    f.push(ChangeEvent::Deleted { id: "r250".into() }).await?;
    let expected = f.sorted(SortDirection::Ascending).await?;
    assert_eq!(expected.len(), 500);
    assert!(shows(&view, &expected).await, "deleted: {:?}", ids(&view));
    assert_eq!(f.lists().await, lists, "no whole-cache list");
    Ok(())
}

/// Forty rows: every fifth lacks the amount, every seventh holds a null,
/// the rest share four amounts (ints and floats alike).
fn forty() -> impl Iterator<Item = (String, Record<CborValue>)> {
    (0..40).map(|n| {
        let id = format!("t{n:02}");
        let amount = match n {
            n if n % 5 == 0 => None,
            n if n % 7 == 0 => Some(CborValue::Null),
            n if n % 2 == 0 => int(n % 4),
            n => Some(CborValue::Float((n % 4) as f64)),
        };
        let r = rec(&id, amount);
        (id, r)
    })
}

#[tokio::test]
async fn a_splice_places_nulls_and_ties_as_a_full_sort_does() -> Result<()> {
    let f = fixture(forty()).await;
    let asc = f.view(SortDirection::Ascending).await?;
    let desc = f.view(SortDirection::Descending).await?;
    let lists = f.lists().await;

    let updated = |id: &str, amount| ChangeEvent::Updated {
        id: id.into(),
        new: Some(rec(id, amount)),
    };
    let inserted = |id: &str, amount| ChangeEvent::Inserted {
        id: id.into(),
        new: Some(rec(id, amount)),
    };
    let changes = [
        updated("t03", int(1)),
        updated("t06", Some(CborValue::Float(2.0))),
        updated("t09", Some(CborValue::Null)),
        updated("t11", None),
        updated("t05", int(3)),
        updated("t14", int(0)),
        inserted("t40", int(2)),
        inserted("t41", Some(CborValue::Null)),
        inserted("t0", None),
        inserted("t99", Some(CborValue::Float(1.0))),
        ChangeEvent::Deleted { id: "t21".into() },
        ChangeEvent::Deleted { id: "t10".into() },
    ];
    for (step, change) in changes.into_iter().enumerate() {
        f.push(change).await?;
        for (view, dir) in [
            (&asc, SortDirection::Ascending),
            (&desc, SortDirection::Descending),
        ] {
            let expected = f.sorted(dir).await?;
            assert!(
                shows(view, &expected).await,
                "step {step} {dir:?}: {:?}",
                ids(view)
            );
        }
    }
    assert_eq!(f.lists().await, lists, "no whole-cache list");
    Ok(())
}
