//! A derived Dio's writes stay inside its full query: the residual the
//! master refuses, and every flash a Lens writer routes.

use std::sync::Arc;

use ciborium::Value as CborValue;
use vantage_core::Result;
use vantage_dataset::traits::WritableValueSet;
use vantage_diorama::{Cache, ChangeFlash, Dio, FlashKind, FlashOutcome, Lens};
use vantage_types::Record;
use vantage_vista::{FilterOp, Vista, mocks::MockShell};

use crate::support::{self, record};
use crate::{residual_shell, team_shell, warmed_dio};

// ---- a derived Dio's writes stay inside its full query ---------------------

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

    let blue = record(&[("id", "4"), ("team", "blue")]);
    assert!(
        facade
            .insert_value("4", &blue)
            .await
            .unwrap_err()
            .is_conflict()
    );
    let moved = ChangeFlash::partial("1", record(&[("team", "blue")]));
    assert!(not_blue.flash(moved).await.unwrap_err().is_conflict());
    let outside = ChangeFlash::delete("3");
    assert!(not_blue.flash(outside).await.unwrap_err().is_conflict());
    let taken = ChangeFlash::replace("3", record(&[("id", "3"), ("team", "red")]));
    assert!(not_blue.flash(taken).await.unwrap_err().is_conflict());

    facade
        .insert_value("5", &record(&[("id", "5"), ("team", "green")]))
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
    red.flash(ChangeFlash::insert("4", record(&[("id", "4")])))
        .await?;
    let missing = red
        .flash(ChangeFlash::partial("3", record(&[("team", "red")])))
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
