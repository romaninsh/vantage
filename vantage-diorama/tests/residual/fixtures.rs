//! The masters and checks the residual tests share.

use std::sync::Arc;

use ciborium::Value as CborValue;
use vantage_core::{Result, error};
use vantage_diorama::{Clause, Dio, LoadState, TableScenery, TableSceneryBuilder};
use vantage_types::Record;
use vantage_vista::mocks::MockShell;
use vantage_vista::{
    Capability, Column, ColumnFlag, Condition, TableShell, Vista, VistaCapabilities, VistaMetadata,
    forward_table_shell,
};

use crate::support::wait_listed;

/// Three rows, `a`, `b` and `c`:
///
/// | id | team | n | dims       | blob   |
/// |----|------|---|------------|--------|
/// | a  | red  | 1 | `{ w: 1 }` | `[1]`  |
/// | b  | blue | 2 | `{ w: 2 }` | `[2]`  |
/// | c  | red  | 3 | `{ w: 3 }` | `[3]`  |
pub fn rows() -> Vec<(String, Record<CborValue>)> {
    [("a", "red", 1), ("b", "blue", 2), ("c", "red", 3)]
        .into_iter()
        .map(|(id, team, n)| {
            let mut row = Record::new();
            row.insert("id".into(), CborValue::Text(id.into()));
            row.insert("team".into(), CborValue::Text(team.into()));
            row.insert("n".into(), CborValue::Integer(n.into()));
            row.insert(
                "dims".into(),
                CborValue::Map(vec![(
                    CborValue::Text("w".into()),
                    CborValue::Integer(n.into()),
                )]),
            );
            row.insert("blob".into(), CborValue::Bytes(vec![n as u8]));
            (id.to_string(), row)
        })
        .collect()
}

/// The schema of [`rows`]: `n` is `Orderable` when `orderable_n`.
pub fn metadata(orderable_n: bool) -> VistaMetadata {
    let n = Column::new("n", "Integer");
    let n = if orderable_n {
        n.with_flag(ColumnFlag::Orderable)
    } else {
        n
    };
    VistaMetadata::new()
        .with_column(Column::new("id", "String").with_flag("id"))
        .with_column(Column::new("team", "String"))
        .with_column(n)
        .with_column(Column::new("dims", "Map"))
        .with_column(Column::new("blob", "Bytes"))
        .with_id_column("id")
}

/// A mock shell over [`rows`] with `metadata`, advertising `Window` and
/// `caps`.
pub fn shell(metadata: VistaMetadata, caps: &[Capability]) -> MockShell {
    let caps: VistaCapabilities = caps.iter().copied().chain([Capability::Window]).collect();
    rows().into_iter().fold(
        MockShell::new()
            .with_metadata(metadata)
            .with_capabilities(caps),
        |shell, (id, row)| shell.with_record(id, row),
    )
}

/// [`rows`] behind a master advertising `Window` and `caps`, with `n` not
/// orderable.
pub fn items(caps: &[Capability]) -> Vista {
    Vista::new("items", shell(metadata(false), caps))
}

/// A shell that can't render a bytes value into its query: a condition on
/// one is refused as unsupported, and every other clause reaches the mock.
#[derive(Clone)]
pub struct NoBytes {
    pub inner: Box<dyn TableShell>,
}

forward_table_shell!(NoBytes, inner, {
    fn add_condition(&mut self, condition: &Condition) -> Result<()> {
        if matches!(condition.value, CborValue::Bytes(_)) {
            return Err(error!(
                "this shell can't render a bytes value",
                column = condition.column.as_str()
            )
            .mark_unsupported());
        }
        self.inner.add_condition(condition)
    }
});

/// The ids a view shows, in row order.
pub fn ids(scenery: &Arc<dyn TableScenery>) -> Vec<String> {
    (0..scenery.row_count())
        .filter_map(|i| scenery.row(i))
        .filter_map(|row| match row.record.get("id") {
            Some(CborValue::Text(id)) => Some(id.clone()),
            _ => None,
        })
        .collect()
}

/// Open a view over `dio` shaped by `build`, and wait for its first answer.
pub async fn open(
    dio: &Dio,
    build: impl FnOnce(TableSceneryBuilder) -> TableSceneryBuilder,
) -> Arc<dyn TableScenery> {
    let scenery = build(dio.table_scenery())
        .open()
        .await
        .expect("scenery opens");
    scenery.set_viewport(0..10, 0..0);
    wait_listed(&scenery).await;
    scenery
}

/// Wait until the view shows exactly `want`, in that order.
pub async fn wait_for_ids(scenery: &Arc<dyn TableScenery>, want: &[&str]) {
    let mut generations = scenery.subscribe();
    let shown = async {
        while ids(scenery) != want {
            generations
                .changed()
                .await
                .expect("generation watch closed");
        }
    };
    if tokio::time::timeout(std::time::Duration::from_secs(3), shown)
        .await
        .is_err()
    {
        panic!("the view shows {:?}, wanted {want:?}", ids(scenery));
    }
}

/// The view has failed on `clause` of table `items`, and shows no rows.
pub fn assert_fails(scenery: &Arc<dyn TableScenery>, clause: Clause) {
    assert_fails_on(scenery, "items", clause);
}

/// [`assert_fails`] over table `table`.
pub fn assert_fails_on(scenery: &Arc<dyn TableScenery>, table: &str, clause: Clause) {
    match scenery.load_state() {
        LoadState::Failed(failure) => {
            assert_eq!(failure.table, table, "{failure}");
            assert_eq!(failure.clause.as_ref(), Some(&clause), "{failure}");
        }
        other => panic!("expected the view to fail on {clause:?}, got {other:?}"),
    }
    assert_eq!(scenery.row_count(), 0, "a failed view shows no rows");
    assert!(scenery.row(0).is_none(), "a failed view shows no rows");
}
