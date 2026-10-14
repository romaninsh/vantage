//! The facade (`dio.cached()`) reads the cache or the narrowed master, and
//! neither applied a derived Dio's residual: every facade read keeps only the
//! rows the residual admits, whichever source it read.

use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_core::Result;
use vantage_dataset::prelude::ReadableValueSet;
use vantage_diorama::{ChangeEvent, Dio};
use vantage_types::Record;
use vantage_vista::{Capability, FilterOp, NativeCondition, Vista};

use super::fixtures::items;
use crate::support::{bucket_dio, cond, eager_dio, record};

/// An eager Dio over [`items`] narrowed by `n > 1`, which a master without
/// `FilterOps` refuses: the residual holds it, and the cache holds a, b, c.
async fn above_one() -> Result<Dio> {
    let parent = eager_dio(items(&[Capability::Order, Capability::Search])).await;
    let dio = parent
        .derive()
        .with_query(&cond("n", FilterOp::Gt, 1))
        .build()
        .await?;
    assert_eq!(dio.residual().conditions.len(), 1, "the master refused it");
    #[cfg(feature = "diagnostics")]
    assert_eq!(dio.diagnostics().await?.cache_rows, 3, "the cache holds a");
    Ok(dio)
}

/// `row` arriving in `dio`'s cache as an upstream change, keyed by its `id`.
async fn listed(dio: &Dio, row: Record<CborValue>) -> Result<()> {
    let id = match row.get("id") {
        Some(CborValue::Text(id)) => id.clone(),
        _ => panic!("a listed row carries its id"),
    };
    dio.apply_change(ChangeEvent::Updated { id, new: Some(row) })
        .await
}

fn keys(rows: &IndexMap<String, Record<CborValue>>) -> Vec<&str> {
    rows.keys().map(String::as_str).collect()
}

/// `list`, `get`, `count` and `fetch_window` of `facade` show exactly
/// `inside`, and never `outside`.
async fn assert_shows(facade: &Vista, inside: &[&str], outside: &str) -> Result<()> {
    assert_eq!(keys(&facade.list_values().await?), inside, "list");
    assert!(facade.get_value(outside).await?.is_none(), "get {outside}");
    for id in inside {
        assert!(facade.get_value(*id).await?.is_some(), "get {id}");
    }
    assert_eq!(facade.count().await?, inside.len() as i64, "count");
    let page = facade.fetch_window(0, 10).await?;
    let paged: Vec<&str> = page.rows.iter().map(|(id, _)| id.as_str()).collect();
    assert_eq!(paged, inside, "fetch_window");
    assert_eq!(page.total, Some(inside.len() as i64), "fetch_window total");
    Ok(())
}

/// Unnarrowed, the facade reads the cache, which holds `a` outside the
/// residual.
#[tokio::test]
async fn an_unnarrowed_facade_applies_the_residual_over_the_cache() -> Result<()> {
    let dio = above_one().await?;
    assert_shows(&dio.cached(), &["b", "c"], "a").await
}

/// Narrowed by a condition the master takes, the facade reads the narrowed
/// master, which never applied the residual either.
#[tokio::test]
async fn a_pushed_down_read_applies_the_residual_over_the_master() -> Result<()> {
    let dio = above_one().await?;
    let red = dio.cached().with_condition("team", FilterOp::Eq, "red")?;
    assert_shows(&red, &["c"], "a").await
}

/// A search the master runs, and a condition it refuses, both still leave
/// the residual applied.
#[tokio::test]
async fn searched_and_refused_reads_apply_the_residual() -> Result<()> {
    let dio = above_one().await?;
    let searched = dio.cached().with_search("red")?;
    assert_shows(&searched, &["c"], "a").await?;

    let small = dio.cached().with_condition("n", FilterOp::Lt, 3)?;
    assert_shows(&small, &["b"], "a").await
}

/// The preview lists the residual among the clauses applied over the cache.
#[tokio::test]
async fn the_preview_names_the_residual() -> Result<()> {
    let dio = above_one().await?;
    let note = dio.cached().preview().note.unwrap_or_default();
    assert!(note.contains("applied over the cache: n > 1"), "{note}");

    let red = dio.cached().with_condition("team", FilterOp::Eq, "red")?;
    let note = red.preview().note.unwrap_or_default();
    assert!(
        note.contains("applied over the master's rows: n > 1"),
        "{note}"
    );
    Ok(())
}

/// A window at an offset is cut from the admitted rows, and its total counts
/// them, not the cache.
#[tokio::test]
async fn a_window_at_an_offset_is_cut_after_the_residual() -> Result<()> {
    let dio = above_one().await?;
    let page = dio.cached().fetch_window(1, 1).await?;
    let ids: Vec<&str> = page.rows.iter().map(|(id, _)| id.as_str()).collect();
    assert_eq!(ids, ["c"]);
    assert_eq!(page.total, Some(2));
    Ok(())
}

/// A cached row still missing its augmented value is hydrated before the
/// residual on that column judges it: never dropped for the gap.
#[tokio::test]
async fn an_unhydrated_row_is_hydrated_before_the_residual_judges_it() -> Result<()> {
    let johns = bucket_dio()
        .await
        .derive()
        .with_query(&cond("name", FilterOp::Eq, "John"))
        .build()
        .await?;
    for (id, kind) in [("o1", "red"), ("o2", "blue"), ("o3", "red")] {
        listed(&johns, record(&[("id", id), ("kind", kind)])).await?;
    }

    let facade = johns.cached();
    assert_eq!(keys(&facade.list_values().await?), ["o1", "o3"]);
    assert_eq!(facade.count().await?, 2);
    assert_eq!(facade.fetch_window(0, 10).await?.total, Some(2));

    listed(&johns, record(&[("id", "o3"), ("kind", "red")])).await?;
    let o3 = facade.get_value("o3").await?.expect("o3 is a John");
    assert_eq!(o3.get("name"), Some(&CborValue::Text("John".into())));
    assert!(facade.get_value("o2").await?.is_none(), "o2 is Jane");
    Ok(())
}

/// An augmented Dio reads its cache whole. A residual on an augmented column
/// is applied over it, alone or under the facade's own clauses.
#[tokio::test]
async fn an_augmented_facade_applies_the_residual() -> Result<()> {
    let johns = bucket_dio()
        .await
        .derive()
        .with_query(&cond("name", FilterOp::Eq, "John"))
        .build()
        .await?;
    assert_eq!(johns.residual().conditions.len(), 1, "kept by diorama");
    for (id, kind, name) in [("o1", "red", "John"), ("o2", "blue", "Jane")] {
        listed(
            &johns,
            record(&[("id", id), ("kind", kind), ("name", name)]),
        )
        .await?;
    }

    assert_shows(&johns.cached(), &["o1"], "o2").await?;
    let blue = johns
        .cached()
        .with_condition("kind", FilterOp::Eq, "blue")?;
    assert!(blue.list_values().await?.is_empty());
    assert!(blue.get_value("o2").await?.is_none());
    Ok(())
}

/// A native condition is opaque to the facade's evaluator, so the facade
/// refuses one rather than accept a clause it would ignore.
#[tokio::test]
async fn the_facade_refuses_a_native_condition() -> Result<()> {
    let dio = above_one().await?;
    let err = dio
        .cached()
        .with_native_condition(NativeCondition::new((), "opaque"))
        .unwrap_err();
    assert!(err.is_unsupported(), "{err}");
    Ok(())
}
