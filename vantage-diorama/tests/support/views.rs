//! Fixtures the owned-view tests share: a small searchable shell, a loader
//! shaped like a REST API, and readers of what a view shows.

use std::ops::Range;
use std::sync::Arc;

use ciborium::Value as CborValue;
use vantage_core::Result;
use vantage_diorama::{ChunkSink, Dio, Loader, LoaderCx, TableScenery};
use vantage_types::Record;
use vantage_vista::mocks::MockShell;
use vantage_vista::{Column, ColumnFlag, Vista, VistaMetadata, VistaQuery};

use super::{memory_lens, record};

/// Rows `a` alpha, `b` beta, `c` gamma; `name` is searchable.
pub fn shell() -> MockShell {
    let metadata = VistaMetadata::new()
        .with_column(Column::new("id", "String").with_flag(ColumnFlag::Id))
        .with_column(Column::new("name", "String").with_flag(ColumnFlag::Searchable))
        .with_id_column("id");
    [("a", "alpha"), ("b", "beta"), ("c", "gamma")]
        .into_iter()
        .fold(
            MockShell::new().with_metadata(metadata),
            |shell, (id, name)| shell.with_record(id, record(&[("id", id), ("name", name)])),
        )
}

/// Shaped like a throttled REST API: one count request and one window request per load.
pub fn rest_loader() -> Loader {
    Loader::paged(serve)
}

/// One load of [`rest_loader`]: count the narrowed master, then read the window.
pub async fn serve(
    cx: LoaderCx,
    range: Range<usize>,
    query: VistaQuery,
    sink: ChunkSink,
) -> Result<()> {
    serve_window(cx, range, query, sink, true).await
}

/// [`serve`], stating the total only when `count` says so: a source that
/// pages without one leaves the set's end a horizon.
pub async fn serve_window(
    cx: LoaderCx,
    range: Range<usize>,
    query: VistaQuery,
    sink: ChunkSink,
    count: bool,
) -> Result<()> {
    let (narrowed, refused) = cx.master().clone().with_query(&query)?;
    if !refused.is_empty() {
        return Err(vantage_core::error!("the master refused part of the query"));
    }
    if count {
        let total = narrowed.count().await?;
        sink.set_total(total.max(0) as usize);
    }
    let page = narrowed.fetch_window(range.start, range.len()).await?;
    for (i, (id, row)) in page.rows.into_iter().enumerate() {
        sink.push(range.start + i, id, row).await?;
    }
    Ok(())
}

/// Row `id` with `n`, or with a null `n`.
pub fn numbered(id: &str, n: Option<i64>) -> Record<CborValue> {
    let mut r = Record::new();
    r.insert("id".into(), CborValue::Text(id.into()));
    r.insert(
        "n".into(),
        n.map_or(CborValue::Null, |n| CborValue::Integer(n.into())),
    );
    r
}

/// A shell holding `rows`, ordered by an orderable `n`.
pub fn numbers_of(rows: &[(&str, Option<i64>)]) -> MockShell {
    let metadata = VistaMetadata::new()
        .with_column(Column::new("id", "String").with_flag(ColumnFlag::Id))
        .with_column(Column::new("n", "i64").with_flag(ColumnFlag::Orderable))
        .with_id_column("id");
    rows.iter().fold(
        MockShell::new().with_metadata(metadata),
        |shell, (id, n)| shell.with_record(*id, numbered(id, *n)),
    )
}

/// The `n` of every row the view shows, in row order.
pub fn ns(view: &Arc<dyn TableScenery>) -> Vec<i64> {
    (0..view.row_count())
        .filter_map(|i| view.row(i))
        .filter_map(|r| match r.record.get("n") {
            Some(CborValue::Integer(n)) => i64::try_from(*n).ok(),
            _ => None,
        })
        .collect()
}

/// The id of every row the view shows, in row order.
pub fn ids(view: &Arc<dyn TableScenery>) -> Vec<String> {
    (0..view.row_count())
        .filter_map(|i| view.row(i))
        .filter_map(|r| {
            r.record
                .get("id")
                .and_then(|v| v.as_text())
                .map(str::to_string)
        })
        .collect()
}

/// A paged Dio over `shell`, loading through `loader`.
pub async fn paged(shell: MockShell, loader: Loader) -> Result<Dio> {
    memory_lens(loader)
        .dio(Vista::new("t", shell))
        .build()
        .await
}
