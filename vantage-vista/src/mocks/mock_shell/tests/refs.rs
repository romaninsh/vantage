use vantage_dataset::ReadableValueSet;

use super::{MockShell, record, text, three_users, user_vista};
use crate::{Column, ColumnFlag, JoinKey, Reference, Vista, VistaMetadata};

#[tokio::test]
async fn get_ref_narrows_the_target_by_its_keys() {
    let orders = MockShell::new()
        .with_record("o1", record(&[("user_id", text("1"))]))
        .with_record("o2", record(&[("user_id", text("2"))]));
    let vista = user_vista(three_users().with_ref_target("orders", orders));
    let alice = record(&[("id", text("1"))]);
    let theirs = vista.get_ref("orders", Some(&alice)).unwrap();
    assert_eq!(theirs.list_values().await.unwrap().len(), 1);
    assert!(theirs.is_narrowed());
    let bare = vista.get_ref("orders", None).unwrap();
    assert_eq!(bare.list_values().await.unwrap().len(), 2);
}

/// A key built with `JoinKey::to_target_id` constrains whatever the target
/// declares as its id column.
#[tokio::test]
async fn a_target_id_key_constrains_the_targets_id_column() {
    let users = MockShell::new()
        .with_metadata(
            VistaMetadata::new()
                .with_column(Column::new("uid", "String").with_flag(ColumnFlag::Id))
                .with_id_column("uid"),
        )
        .with_record("1", record(&[("uid", text("1"))]))
        .with_record("2", record(&[("uid", text("2"))]));
    let metadata = VistaMetadata::new()
        .with_column(Column::new("id", "String").with_flag(ColumnFlag::Id))
        .with_column(Column::new("user_id", "String"))
        .with_id_column("id")
        .with_reference(Reference::has_one(
            "user",
            "users",
            JoinKey::to_target_id("user_id"),
        ));
    let orders = Vista::new(
        "orders",
        MockShell::new()
            .with_metadata(metadata)
            .with_ref_target("user", users),
    );
    assert_eq!(
        orders.reference("user").unwrap().foreign_key(),
        Some("user_id")
    );

    let order = record(&[("id", text("o1")), ("user_id", text("2"))]);
    let user = orders.get_ref("user", Some(&order)).unwrap();
    assert_eq!(user.query().conditions[0].column, "uid");
    let rows = user.list_values().await.unwrap();
    assert_eq!(rows.keys().collect::<Vec<_>>(), ["2"]);
}

#[test]
fn a_join_key_without_to_reads_as_the_target_id() {
    let key: JoinKey = serde_json::from_str(r#"{"from": "user_id"}"#).unwrap();
    assert_eq!(key, JoinKey::to_target_id("user_id"));
    assert_eq!(
        serde_json::to_string(&key).unwrap(),
        r#"{"from":"user_id"}"#
    );
    let full: JoinKey = serde_json::from_str(r#"{"from": "id", "to": "user_id"}"#).unwrap();
    assert_eq!(full, JoinKey::new("id", "user_id"));
}
