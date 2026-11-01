//! A narrowed Dio facade writes only inside its own set.

mod derived;
mod dotted;
mod facade;
#[path = "../support/mod.rs"]
mod support;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_core::{Result, error};
use vantage_dataset::traits::WritableValueSet;
use vantage_diorama::{ChangeFlash, Dio};
use vantage_types::Record;
use vantage_vista::{
    Capability, Column, Denials, FilterOp, Page, TableShell, Vista, VistaMetadata,
    forward_table_shell, mocks::MockShell,
};

use support::record;

/// Teams 1 red, 2 red, 3 blue; cache warmed from the master. Returns the
/// master's shell so a test can read the store directly.
async fn teams() -> Result<(Dio, MockShell)> {
    let shell = team_shell();
    let dio = warmed_dio(Vista::new("teams", shell.clone())).await?;
    Ok((dio, shell))
}

fn team_shell() -> MockShell {
    MockShell::new()
        .with_metadata(
            VistaMetadata::new()
                .with_column(Column::new("id", "String").with_flag("id"))
                .with_column(Column::new("team", "String"))
                .with_id_column("id"),
        )
        .with_record("1", record(&[("id", "1"), ("team", "red")]))
        .with_record("2", record(&[("id", "2"), ("team", "red")]))
        .with_record("3", record(&[("id", "3"), ("team", "blue")]))
}

/// The team shell, writable and confining, but without `FilterOps`: a `Ne`
/// condition is refused, so a facade reads its set from the cache and a
/// derived Dio keeps the condition as its residual.
fn residual_shell() -> MockShell {
    team_shell().with_capabilities(
        [
            Capability::Insert,
            Capability::Update,
            Capability::Delete,
            Capability::ConfineWrites,
        ]
        .into_iter()
        .collect(),
    )
}

async fn warmed_dio(master: Vista) -> Result<Dio> {
    Ok(support::eager_dio(master).await)
}

/// A shell that refuses every read of the whole set once `refuse` is set,
/// while point reads and writes pass through.
#[derive(Clone)]
struct ListRefusingShell {
    inner: Box<dyn TableShell>,
    refuse: Arc<AtomicBool>,
}

impl ListRefusingShell {
    fn check(&self) -> Result<()> {
        if self.refuse.load(Ordering::SeqCst) {
            return Err(error!("listing refused"));
        }
        Ok(())
    }
}

forward_table_shell!(ListRefusingShell, inner, {
    async fn list(&self) -> Result<IndexMap<String, Record<CborValue>>> {
        self.check()?;
        self.inner.list().await
    }
    async fn count(&self) -> Result<i64> {
        self.check()?;
        self.inner.count().await
    }
    async fn fetch_window(&self, offset: usize, limit: usize) -> Result<Page> {
        self.check()?;
        self.inner.fetch_window(offset, limit).await
    }
});

/// A narrowed `Clear` runs as one delete per row, yet it is still a bulk
/// delete: with `delete_all` denied it is refused, while a single delete
/// lands.
#[tokio::test]
async fn narrowed_clear_needs_delete_all() -> Result<()> {
    let mut denials = Denials::new();
    denials.deny(Capability::DeleteAll, "policy/crm.yaml#no-bulk-delete");
    let shell = residual_shell().with_denials(denials);
    let dio = warmed_dio(Vista::new("teams", shell.clone())).await?;
    let not_blue = dio
        .derive()
        .with_query(&support::cond("team", FilterOp::Ne, "blue"))
        .build()
        .await?;
    assert!(
        !not_blue.residual().conditions.is_empty(),
        "runs as deletes"
    );

    let err = not_blue.flash(ChangeFlash::clear()).await.unwrap_err();
    assert!(err.is_forbidden(), "{err}");
    assert_eq!(shell.record_ids().len(), 3, "the clear wrote nothing");

    not_blue.flash(ChangeFlash::delete("1")).await?;
    assert!(shell.get_record("1").is_none());
    Ok(())
}

/// `id` is outside the red set: a patch into it is NotFound, an insert
/// under the id a Conflict.
async fn assert_outside_red(red: &Vista, id: &str) {
    assert!(
        red.patch_value(id, &record(&[("team", "red")]))
            .await
            .unwrap_err()
            .is_not_found()
    );
    assert!(
        red.insert_value(id, &record(&[("id", id)]))
            .await
            .unwrap_err()
            .is_conflict()
    );
}
