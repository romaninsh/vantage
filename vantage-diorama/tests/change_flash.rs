//! `ChangeFlash` — the immutable outbound change, mount → vista.
//!
//! Mirror of the inbound `ChangeEvent`: everything the write pipeline,
//! routing callbacks, and audit trails need travels in one frozen value —
//! `kind`, `id`, the `patch` (only the fields that changed), the `before`
//! pre-image, and the derived `after` merge.

use ciborium::Value as CborValue;
use vantage_core::Result;
use vantage_dataset::traits::ReadableValueSet;
use vantage_diorama::{ChangeFlash, FlashKind};
use vantage_types::Record;
use vantage_vista::{Column, Vista, VistaMetadata, mocks::MockShell};

fn text(s: &str) -> CborValue {
    CborValue::Text(s.to_string())
}

fn rec(pairs: &[(&str, CborValue)]) -> Record<CborValue> {
    let mut r = Record::new();
    for (k, v) in pairs {
        r.insert((*k).to_string(), v.clone());
    }
    r
}

#[test]
fn patch_flash_derives_after_from_before_plus_patch() {
    let flash = ChangeFlash::new(
        FlashKind::Patch,
        Some("c1".to_string()),
        rec(&[("email", text("new@example.com"))]),
    )
    .with_before(rec(&[
        ("email", text("old@example.com")),
        ("plan", text("pro")),
    ]));

    assert_eq!(flash.kind(), &FlashKind::Patch);
    assert_eq!(flash.id(), Some("c1"));
    assert_eq!(flash.patch().len(), 1);

    let after = flash.after().expect("a patch has a merged result");
    assert_eq!(after.get("email"), Some(&text("new@example.com")));
    assert_eq!(
        after.get("plan"),
        Some(&text("pro")),
        "untouched field survives the merge"
    );
}

#[test]
fn insert_flash_has_no_before_and_after_is_the_record() {
    let flash = ChangeFlash::insert("c2", rec(&[("name", text("Ada"))]));

    assert_eq!(flash.kind(), &FlashKind::Insert);
    assert!(flash.before().is_none());
    assert_eq!(flash.after().unwrap().get("name"), Some(&text("Ada")));
}

#[test]
fn replace_flash_after_is_authoritative_not_merged() {
    // Replace exists because a patch-merge cannot express field removal.
    let flash = ChangeFlash::replace("c1", rec(&[("name", text("Ada"))]))
        .with_before(rec(&[("name", text("Al")), ("nickname", text("Big Al"))]));

    let after = flash.after().unwrap();
    assert_eq!(after.get("name"), Some(&text("Ada")));
    assert!(
        after.get("nickname").is_none(),
        "replace drops absent fields"
    );
}

#[test]
fn delete_flash_carries_the_pre_image_and_no_after() {
    let flash = ChangeFlash::delete("c1").with_before(rec(&[("name", text("Ada"))]));

    assert_eq!(flash.kind(), &FlashKind::Delete);
    assert!(flash.after().is_none());
    assert_eq!(
        flash.before().and_then(|b| b.get("name").cloned()),
        Some(text("Ada")),
        "audit/routing can see what was deleted"
    );
}

#[test]
fn clear_flash_is_idless() {
    let flash = ChangeFlash::clear();
    assert_eq!(flash.kind(), &FlashKind::Clear);
    assert!(flash.id().is_none());
    assert!(flash.after().is_none());
}

// ---- active_record: bind the change to a different datasource ---------------

fn shadow_vista(shell: &MockShell) -> Vista {
    let metadata = VistaMetadata::new()
        .with_column(Column::new("id", "String").with_flag("id"))
        .with_id_column("id");
    Vista::new("shadow", Box::new(shell.clone().with_metadata(metadata)))
}

#[tokio::test]
async fn active_record_routes_the_merged_record_into_another_dataset() -> Result<()> {
    let shadow_shell = MockShell::new();
    let shadow = shadow_vista(&shadow_shell);

    let flash = ChangeFlash::new(
        FlashKind::Patch,
        Some("c1".to_string()),
        rec(&[("status", text("won"))]),
    )
    .with_before(rec(&[("status", text("lead")), ("name", text("Ada"))]));

    // The routing ergonomic: same record concept, different datasource.
    let record = flash.active_record(&shadow)?;
    record.save().await?;

    let landed = shadow.get_value("c1").await?.expect("routed row landed");
    assert_eq!(landed.get("status"), Some(&text("won")));
    assert_eq!(
        landed.get("name"),
        Some(&text("Ada")),
        "the AFTER image lands, not the bare patch"
    );
    Ok(())
}

#[tokio::test]
async fn active_record_refuses_a_flash_with_no_record_shape() {
    let shadow_shell = MockShell::new();
    let shadow = shadow_vista(&shadow_shell);

    assert!(
        ChangeFlash::delete("c1").active_record(&shadow).is_err(),
        "a delete has no merged record to bind"
    );
    assert!(ChangeFlash::clear().active_record(&shadow).is_err());
}
