//! A narrowed Dio facade writes only inside its own set.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_core::{Result, error};
use vantage_dataset::traits::WritableValueSet;
use vantage_diorama::{Cache, ChangeFlash, Dio, FlashKind, FlashOutcome, Lens};
use vantage_types::Record;
use vantage_vista::{
    Column, FilterOp, Page, TableShell, Vista, VistaMetadata, forward_table_shell, mocks::MockShell,
};

mod support;

fn rec(pairs: &[(&str, &str)]) -> Record<CborValue> {
    let mut r = Record::new();
    for (k, v) in pairs {
        r.insert((*k).to_string(), CborValue::Text((*v).to_string()));
    }
    r
}

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
        .with_record("1", rec(&[("id", "1"), ("team", "red")]))
        .with_record("2", rec(&[("id", "2"), ("team", "red")]))
        .with_record("3", rec(&[("id", "3"), ("team", "blue")]))
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

#[tokio::test]
async fn narrowed_delete_all_keeps_rows_outside_the_set() -> Result<()> {
    let (dio, shell) = teams().await?;
    let mut red = dio.cached();
    red.add_condition("team", FilterOp::Eq, "red")?;

    red.delete_all().await?;

    assert_eq!(
        shell.record_ids(),
        vec!["3".to_string()],
        "only the red rows go"
    );
    Ok(())
}

/// `id` is outside the red set: a patch into it is NotFound, an insert
/// under the id a Conflict.
async fn assert_outside_red(red: &Vista, id: &str) {
    assert!(
        red.patch_value(id, &rec(&[("team", "red")]))
            .await
            .unwrap_err()
            .is_not_found()
    );
    assert!(
        red.insert_value(id, &rec(&[("id", id)]))
            .await
            .unwrap_err()
            .is_conflict()
    );
}

#[tokio::test]
async fn narrowed_facade_writes_stay_in_the_set() -> Result<()> {
    let (dio, shell) = teams().await?;
    let mut red = dio.cached();
    red.add_condition("team", FilterOp::Eq, "red")?;

    red.delete("3").await?; // blue: outside the set
    assert_outside_red(&red, "3").await;
    red.insert_value("4", &rec(&[("id", "4")])).await?; // team filled
    red.delete("ghost").await?; // idempotent
    assert!(shell.get_record("3").is_some(), "blue row untouched");
    assert_eq!(
        shell.get_record("4").unwrap()["team"],
        CborValue::Text("red".into())
    );
    Ok(())
}

/// A narrowed by-id write checks membership with a point read of the one id,
/// never by listing the set: with every master listing refused, writes
/// still resolve — inside the set or outside it.
#[tokio::test]
async fn narrowed_writes_read_one_row_not_the_set() -> Result<()> {
    let refuse = Arc::new(AtomicBool::new(false));
    let master = Vista::new("teams", team_shell()).map_shell(|inner| ListRefusingShell {
        inner,
        refuse: refuse.clone(),
    });
    let dio = warmed_dio(master).await?;
    let mut red = dio.cached();
    red.add_condition("team", FilterOp::Eq, "red")?;
    refuse.store(true, Ordering::SeqCst);

    red.patch_value("1", &rec(&[("team", "red")])).await?;
    red.delete("2").await?;
    red.delete("3").await?; // blue, cached: outside, nothing queued
    assert_outside_red(&red, "3").await;
    Ok(())
}

/// Rows the cache has not seen are looked up in the master, then judged
/// against the narrowing like cached ones.
#[tokio::test]
async fn narrowed_writes_find_rows_only_the_master_holds() -> Result<()> {
    let (dio, shell) = teams().await?;
    let mut red = dio.cached();
    red.add_condition("team", FilterOp::Eq, "red")?;
    shell.set_record("5", rec(&[("id", "5"), ("team", "red")]));
    shell.set_record("6", rec(&[("id", "6"), ("team", "blue")]));

    red.patch_value("5", &rec(&[("team", "red")])).await?;
    assert_outside_red(&red, "6").await;
    red.delete("6").await?; // outside: nothing queued
    red.delete("5").await?;

    assert!(shell.get_record("5").is_none(), "red row deleted");
    assert!(shell.get_record("6").is_some(), "blue row untouched");
    Ok(())
}

/// The master moved row 1 out of the set; the cache still shows it red. A
/// narrowed write judges membership from the master, so it leaves row 1 alone.
#[tokio::test]
async fn narrowed_write_ignores_a_stale_cache_row() -> Result<()> {
    let (dio, shell) = teams().await?;
    let mut red = dio.cached();
    red.add_condition("team", FilterOp::Eq, "red")?;
    shell.set_field("1", "team", CborValue::Text("blue".into()));

    assert_outside_red(&red, "1").await;
    red.delete("1").await?;
    red.delete("2").await?;

    assert_eq!(
        shell.get_record("1").unwrap()["team"],
        CborValue::Text("blue".into())
    );
    Ok(())
}

/// A handle whose condition the master refuses reads its set from the cache.
/// `delete_all` judges each id as a single delete does, against the master,
/// so a stale cached row the master has moved out of the set is kept.
#[tokio::test]
async fn narrowed_delete_all_skips_a_stale_cached_row() -> Result<()> {
    use vantage_vista::Capability;
    let shell = team_shell().with_capabilities(
        [
            Capability::Insert,
            Capability::Update,
            Capability::Delete,
            Capability::ConfineWrites,
        ]
        .into_iter()
        .collect(),
    );
    let dio = warmed_dio(Vista::new("teams", shell.clone())).await?;
    let mut not_blue = dio.cached();
    not_blue.add_condition("team", FilterOp::Ne, "blue")?;
    shell.set_field("1", "team", CborValue::Text("blue".into()));

    not_blue.delete_all().await?;

    assert!(shell.get_record("2").is_none(), "red row 2 deleted");
    assert!(
        shell.get_record("1").is_some(),
        "row 1 left the set upstream"
    );
    assert!(shell.get_record("3").is_some(), "blue row untouched");
    Ok(())
}

fn address(city: &str) -> CborValue {
    CborValue::Map(vec![(
        CborValue::Text("city".into()),
        CborValue::Text(city.into()),
    )])
}

/// People with a nested `address` map; person 1 lives in Riga. Returns the
/// facade narrowed to `address.city == "Riga"` and the master's shell.
async fn riga_people() -> Result<(Vista, MockShell)> {
    let shell = MockShell::new().with_metadata(
        VistaMetadata::new()
            .with_column(Column::new("id", "String").with_flag("id"))
            .with_column(Column::new("name", "String"))
            .with_column(Column::new("address", "Map"))
            .with_id_column("id"),
    );
    let mut row = rec(&[("id", "1"), ("name", "Ada")]);
    row.insert("address".into(), address("Riga"));
    shell.set_record("1", row);
    let dio = warmed_dio(Vista::new("people", shell.clone())).await?;
    let mut riga = dio.cached();
    riga.add_condition("address.city", FilterOp::Eq, "Riga")?;
    Ok((riga, shell))
}

/// A set narrowed on a dotted path (`address.city`) has no flat cell to fill:
/// a write must hold the value at the path itself, and nothing writes the
/// literal key `"address.city"`.
#[tokio::test]
async fn dotted_path_narrowing_fills_no_literal_key() -> Result<()> {
    let (riga, shell) = riga_people().await?;

    let no_address = riga.insert_value("2", &rec(&[("id", "2")])).await;
    assert!(no_address.unwrap_err().is_conflict(), "nothing to fill");
    let mut elsewhere = rec(&[("id", "3")]);
    elsewhere.insert("address".into(), address("Oslo"));
    assert!(
        riga.insert_value("3", &elsewhere)
            .await
            .unwrap_err()
            .is_conflict()
    );
    let mut moved = Record::new();
    moved.insert("address".into(), address("Oslo"));
    assert!(
        riga.patch_value("1", &moved)
            .await
            .unwrap_err()
            .is_conflict()
    );

    let mut inside = rec(&[("id", "4")]);
    inside.insert("address".into(), address("Riga"));
    riga.insert_value("4", &inside).await?;
    let stored = shell.get_record("4").expect("row 4 written");
    assert!(stored.get("address.city").is_none(), "{stored:?}");
    assert_eq!(stored.get("address"), Some(&address("Riga")));
    Ok(())
}

/// A literal `"address.city"` key in a written record can't stand in for the
/// nested value: the set check walks the path.
#[tokio::test]
async fn dotted_path_set_ignores_a_literal_dotted_key() -> Result<()> {
    let (riga, _shell) = riga_people().await?;
    let mut disguised = rec(&[("id", "5"), ("address.city", "Riga")]);
    disguised.insert("address".into(), address("Oslo"));
    assert!(
        riga.insert_value("5", &disguised)
            .await
            .unwrap_err()
            .is_conflict()
    );
    let mut disguised_patch = rec(&[("address.city", "Riga")]);
    disguised_patch.insert("address".into(), address("Oslo"));
    assert!(
        riga.patch_value("1", &disguised_patch)
            .await
            .unwrap_err()
            .is_conflict()
    );
    Ok(())
}

/// A patch that keeps the row in the dotted set goes through.
#[tokio::test]
async fn dotted_path_set_takes_an_in_set_patch() -> Result<()> {
    let (riga, shell) = riga_people().await?;
    riga.patch_value("1", &rec(&[("name", "Ada L.")])).await?;
    let stored = shell.get_record("1").expect("row 1");
    assert_eq!(stored.get("name"), Some(&CborValue::Text("Ada L.".into())));
    assert_eq!(stored.get("address"), Some(&address("Riga")));
    Ok(())
}

#[tokio::test]
async fn retried_insert_flash_is_a_no_op() -> Result<()> {
    let (dio, shell) = teams().await?;
    let insert = || ChangeFlash::insert("9", rec(&[("id", "9"), ("team", "red")]));
    dio.flash(insert()).await?;
    dio.flash(insert()).await?;
    assert_eq!(shell.len(), 4);
    Ok(())
}

// ---- a derived Dio's writes stay inside its full query ---------------------

/// The team shell, writable and confining, but without `FilterOps`: a `Ne`
/// condition is refused and becomes a derived Dio's residual.
fn residual_shell() -> MockShell {
    use vantage_vista::Capability;
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

/// A Dio over the team shell narrowed to `team != blue`, a clause its master
/// refuses: the master holds every row and the residual does the narrowing.
async fn not_blue() -> Result<(Dio, MockShell)> {
    let shell = residual_shell();
    let dio = warmed_dio(Vista::new("teams", shell.clone())).await?;
    let not_blue = dio
        .derive()
        .with_query(&support::cond("team", FilterOp::Ne, "blue"))
        .build()
        .await?;
    assert!(!not_blue.residual().conditions.is_empty());
    Ok((not_blue, shell))
}

/// `delete_all` on a derived Dio's facade deletes only the rows its residual
/// admits.
#[tokio::test]
async fn derived_delete_all_keeps_rows_outside_the_residual() -> Result<()> {
    let (not_blue, shell) = not_blue().await?;
    not_blue.cached().delete_all().await?;
    assert_eq!(shell.record_ids(), vec!["3".to_string()]);
    Ok(())
}

/// Facade and Dio writes on a derived Dio are held to its residual before
/// anything is staged.
#[tokio::test]
async fn derived_writes_are_refused_outside_the_residual() -> Result<()> {
    let (not_blue, shell) = not_blue().await?;
    let facade = not_blue.cached();

    let blue = rec(&[("id", "4"), ("team", "blue")]);
    assert!(
        facade
            .insert_value("4", &blue)
            .await
            .unwrap_err()
            .is_conflict()
    );
    let moved = ChangeFlash::partial("1", rec(&[("team", "blue")]));
    assert!(not_blue.flash(moved).await.unwrap_err().is_conflict());
    let outside = ChangeFlash::delete("3");
    assert!(not_blue.flash(outside).await.unwrap_err().is_conflict());
    let taken = ChangeFlash::replace("3", rec(&[("id", "3"), ("team", "red")]));
    assert!(not_blue.flash(taken).await.unwrap_err().is_conflict());

    facade
        .insert_value("5", &rec(&[("id", "5"), ("team", "green")]))
        .await?;
    assert_eq!(shell.len(), 4, "only the in-set insert landed");
    assert_eq!(
        shell.get_record("1").unwrap()["team"],
        CborValue::Text("red".into())
    );
    assert!(shell.get_record("3").is_some());
    Ok(())
}

/// What a recording writer saw: the kind, the id and the record of each flash.
type Seen = Arc<std::sync::Mutex<Vec<(FlashKind, Option<String>, Record<CborValue>)>>>;

/// A Dio over the team shell narrowed to `team == red`, which the master
/// takes, whose Lens writer records every flash and writes nothing.
async fn routed_red() -> Result<(Dio, Seen)> {
    let seen = Seen::default();
    let lens = Lens::builder()
        .cache(Cache::memory())
        .loader(support::copy_master())
        .writer({
            let seen = seen.clone();
            move |_cx, flash| {
                let entry = (
                    *flash.kind(),
                    flash.id().map(str::to_string),
                    flash.patch().clone(),
                );
                seen.lock().unwrap().push(entry);
                async { Ok(FlashOutcome::applied()) }
            }
        })
        .build()?;
    let dio = lens.dio(Vista::new("teams", team_shell())).build().await?;
    let red = dio
        .derive()
        .with_query(&support::cond("team", FilterOp::Eq, "red"))
        .build()
        .await?;
    assert!(red.residual().is_empty(), "the master took the condition");
    Ok((red, seen))
}

/// A writer bypasses the master's own confinement, so a Dio with a writer
/// checks its whole query: a `Clear` becomes one delete per row in the set,
/// an insert gets the set's value filled in, and a row outside the set is
/// never written.
#[tokio::test]
async fn a_writer_route_is_confined_to_the_full_query() -> Result<()> {
    let (red, seen) = routed_red().await?;

    red.cached().delete_all().await?;
    red.flash(ChangeFlash::insert("4", rec(&[("id", "4")])))
        .await?;
    let missing = red
        .flash(ChangeFlash::partial("3", rec(&[("team", "red")])))
        .await;
    assert!(missing.unwrap_err().is_not_found(), "3 is not in the set");
    red.flash(ChangeFlash::delete("3")).await?;

    let seen = seen.lock().unwrap();
    let flashes: Vec<String> = seen
        .iter()
        .map(|(kind, id, _)| format!("{kind:?} {}", id.as_deref().unwrap_or("-")))
        .collect();
    assert_eq!(flashes, ["Delete 1", "Delete 2", "Insert 4"]);
    assert_eq!(seen[2].2.get("team"), Some(&CborValue::Text("red".into())));
    Ok(())
}
