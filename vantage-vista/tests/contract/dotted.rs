//! The dotted-path probe: a condition on `root.leaf` into a declared column
//! is refused or narrows, never a plain error and never accepted and
//! matched against nothing.

use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_core::Result;
use vantage_types::Record;
use vantage_vista::{
    Capability, Column, ColumnFlag, Condition, TableShell, Vista, VistaMetadata, contract,
    mocks::MockShell,
};

use super::{cmap, rec, t};

type Rows = IndexMap<String, Record<CborValue>>;

/// Two boxes whose `dims` column holds a nested map.
fn boxes() -> MockShell {
    let metadata = VistaMetadata::new()
        .with_column(Column::new("id", "String").with_flag(ColumnFlag::Id))
        .with_column(Column::new("dims", "map"))
        .with_id_column("id");
    let dims = |w: &str| cmap(&[("w", t(w))]);
    MockShell::new()
        .with_metadata(metadata)
        .with_capabilities(
            [Capability::FilterOps, Capability::Window]
                .into_iter()
                .collect(),
        )
        .with_record("b1", rec(&[("id", t("b1")), ("dims", dims("wide"))]))
        .with_record("b2", rec(&[("id", t("b2")), ("dims", dims("narrow"))]))
}

/// The mock refuses dotted paths with `condition_column_error`, which passes.
#[tokio::test]
async fn mock_contract_narrowing_accepts_a_refused_dotted_path() {
    contract::check_narrowing(&Vista::new("box", boxes())).await;
}

/// Rejects a dotted condition with a plain error instead of refusing it.
#[derive(Clone)]
struct Plain {
    inner: Box<dyn TableShell>,
}

vantage_vista::forward_table_shell!(Plain, inner, {
    fn add_condition(&mut self, condition: &Condition) -> Result<()> {
        if condition.column.contains('.') {
            return Err(vantage_core::error!("no paths here"));
        }
        self.inner.add_condition(condition)
    }
});

#[tokio::test]
#[should_panic(expected = "plain error")]
async fn mock_contract_narrowing_catches_a_plain_error_on_a_dotted_path() {
    let vista = Vista::new("box", boxes()).map_shell(|inner| Plain { inner });
    contract::check_narrowing(&vista).await;
}

/// Evaluates `root.leaf == value` against the nested map; other conditions
/// go to the inner shell.
#[derive(Clone)]
struct Nested {
    inner: Box<dyn TableShell>,
    paths: Vec<(String, String, CborValue)>,
}

vantage_vista::forward_table_shell!(Nested, inner, {
    fn add_condition(&mut self, condition: &Condition) -> Result<()> {
        match condition.column.split_once('.') {
            Some((root, leaf)) => {
                let path = (root.to_string(), leaf.to_string(), condition.value.clone());
                self.paths.push(path);
                Ok(())
            }
            None => self.inner.add_condition(condition),
        }
    }
    async fn list(&self) -> Result<Rows> {
        let mut rows = self.inner.list().await?;
        rows.retain(|_, row| {
            self.paths
                .iter()
                .all(|(root, leaf, value)| match row.get(root) {
                    Some(CborValue::Map(entries)) => entries.contains(&(t(leaf), value.clone())),
                    _ => false,
                })
        });
        Ok(rows)
    }
});

#[tokio::test]
async fn mock_contract_narrowing_accepts_an_evaluated_dotted_path() {
    let vista = Vista::new("box", boxes()).map_shell(|inner| Nested {
        inner,
        paths: Vec::new(),
    });
    contract::check_narrowing(&vista).await;
}

/// Takes a dotted condition and then lists no rows at all.
#[derive(Clone)]
struct Blind {
    inner: Box<dyn TableShell>,
    blind: bool,
}

vantage_vista::forward_table_shell!(Blind, inner, {
    fn add_condition(&mut self, condition: &Condition) -> Result<()> {
        if condition.column.contains('.') {
            self.blind = true;
            return Ok(());
        }
        self.inner.add_condition(condition)
    }
    async fn list(&self) -> Result<Rows> {
        if self.blind {
            return Ok(Rows::new());
        }
        self.inner.list().await
    }
});

#[tokio::test]
#[should_panic(expected = "not match nothing")]
async fn mock_contract_narrowing_catches_a_dotted_path_matching_nothing() {
    let vista = Vista::new("box", boxes()).map_shell(|inner| Blind {
        inner,
        blind: false,
    });
    contract::check_narrowing(&vista).await;
}

/// Takes a dotted condition and ignores it, so every row still lists.
#[derive(Clone)]
struct Ignore {
    inner: Box<dyn TableShell>,
}

vantage_vista::forward_table_shell!(Ignore, inner, {
    fn add_condition(&mut self, condition: &Condition) -> Result<()> {
        if condition.column.contains('.') {
            return Ok(());
        }
        self.inner.add_condition(condition)
    }
});

#[tokio::test]
#[should_panic(expected = "still lists row b2, which lacks that value: \
                           the dotted path is accepted but not applied")]
async fn mock_contract_narrowing_catches_an_ignored_dotted_path() {
    let vista = Vista::new("box", boxes()).map_shell(|inner| Ignore { inner });
    contract::check_narrowing(&vista).await;
}
