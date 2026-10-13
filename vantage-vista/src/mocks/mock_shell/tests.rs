use ciborium::Value as CborValue;
use vantage_dataset::{InsertableValueSet, ReadableValueSet, WritableValueSet};
use vantage_types::Record;

use super::MockShell;
use crate::{
    Capability, Column, ColumnFlag, FilterOp, JoinKey, Reference, Vista, VistaCapabilities,
    VistaMetadata,
};

mod conditions;
mod get_only;

fn text(s: &str) -> CborValue {
    CborValue::Text(s.into())
}

fn record(pairs: &[(&str, CborValue)]) -> Record<CborValue> {
    pairs
        .iter()
        .map(|(k, v)| ((*k).to_string(), v.clone()))
        .collect()
}

fn user_vista(source: MockShell) -> Vista {
    let metadata = VistaMetadata::new()
        .with_column(Column::new("id", "String").with_flag(ColumnFlag::Id))
        .with_column(Column::new("name", "String").with_flag(ColumnFlag::Title))
        .with_column(Column::new("email", "String").with_flag(ColumnFlag::Hidden))
        .with_column(Column::new("vip_flag", "bool"))
        .with_id_column("id")
        .with_reference(Reference::has_many(
            "orders",
            "orders",
            JoinKey::new("id", "user_id"),
        ));
    Vista::new("users", source.with_metadata(metadata))
}

fn three_users() -> MockShell {
    let user =
        |name: &str, vip: bool| record(&[("name", text(name)), ("vip_flag", CborValue::Bool(vip))]);
    MockShell::new()
        .with_record("1", user("Alice", true))
        .with_record("2", user("Bob", false))
        .with_record("3", user("Carol", true))
}

#[test]
fn metadata_accessors_round_trip() {
    let vista = user_vista(MockShell::new());
    assert_eq!(vista.name(), "users");
    assert_eq!(vista.id_column(), Some("id"));
    assert_eq!(vista.title_columns(), vec!["name"]);
    let names: Vec<&str> = vista.columns().keys().map(String::as_str).collect();
    assert_eq!(names, ["id", "name", "email", "vip_flag"]);
    assert!(vista.column("email").unwrap().has_flag(ColumnFlag::Hidden));
    assert_eq!(
        vista.reference("orders").unwrap().foreign_key(),
        Some("user_id")
    );
    let caps = vista.capabilities();
    assert!(caps.has(Capability::Insert) && caps.has(Capability::Window));
    assert!(!caps.has(Capability::Subscribe));
}

#[tokio::test]
async fn reads_return_seeded_rows() {
    let vista = user_vista(three_users());
    let rows = vista.list_values().await.unwrap();
    assert_eq!(rows.len(), 3);
    assert_eq!(rows["2"].get("name"), Some(&text("Bob")));
    let alice = vista.get_value("1").await.unwrap().unwrap();
    assert_eq!(alice.get("name"), Some(&text("Alice")));
    assert_eq!(vista.count().await.unwrap(), 3);
    let page = vista.fetch_window(1, 1).await.unwrap();
    assert_eq!(page.rows[0].0, "2");
    assert_eq!(page.total, Some(3));
}

#[tokio::test]
async fn conditions_filter_every_read_and_are_recorded() {
    let vista = user_vista(three_users())
        .with_condition("vip_flag", FilterOp::Eq, true)
        .unwrap();
    let rows = vista.list_values().await.unwrap();
    assert_eq!(rows.keys().collect::<Vec<_>>(), ["1", "3"]);
    assert_eq!(vista.count().await.unwrap(), 2);
    assert!(vista.get_value("2").await.unwrap().is_none());
    assert!(vista.is_narrowed());
    assert_eq!(vista.query().conditions.len(), 1);

    let named = vista.with_condition("name", FilterOp::Gt, "B").unwrap();
    assert_eq!(named.list_values().await.unwrap().len(), 1);
}

#[tokio::test]
async fn order_and_search_apply_and_clear() {
    let refused = user_vista(three_users()).with_order("name", crate::SortDirection::Descending);
    assert!(
        refused.unwrap_err().is_unsupported(),
        "name isn't orderable"
    );

    let mut vista = user_vista(three_users());
    vista.set_search(Some("o")).unwrap();
    assert_eq!(vista.list_values().await.unwrap().len(), 2, "Bob, Carol");
    vista.set_search(None).unwrap();
    assert!(vista.query().search.is_none());
    assert_eq!(vista.list_values().await.unwrap().len(), 3);
}

#[tokio::test]
async fn writable_value_set_round_trip() {
    let vista = user_vista(MockShell::new());

    let inserted = vista
        .insert_value("alice", &record(&[("name", text("Alice"))]))
        .await
        .unwrap();
    assert_eq!(inserted.get("id"), Some(&text("alice")));

    // a retried insert returns the stored row untouched
    let dup = vista.insert_value("alice", &record(&[])).await.unwrap();
    assert_eq!(dup.get("name"), Some(&text("Alice")));

    vista
        .replace_value("alice", &record(&[("name", text("Alicia"))]))
        .await
        .unwrap();
    vista
        .patch_value("alice", &record(&[("email", text("a@example.com"))]))
        .await
        .unwrap();
    let patched = vista.get_value("alice").await.unwrap().unwrap();
    assert_eq!(patched.get("name"), Some(&text("Alicia")));
    assert_eq!(patched.get("email"), Some(&text("a@example.com")));

    vista
        .upsert_value("bob", &record(&[("name", text("Bob"))]))
        .await
        .unwrap();
    assert!(vista.get_value("bob").await.unwrap().is_some());

    vista.delete("alice").await.unwrap();
    assert!(vista.get_value("alice").await.unwrap().is_none());
    vista.delete("alice").await.unwrap();

    vista.delete_all().await.unwrap();
    assert!(vista.list_values().await.unwrap().is_empty());
}

#[tokio::test]
async fn insertable_value_set_assigns_ids() {
    let vista = user_vista(MockShell::new());
    let auto_id = vista
        .insert_return_id_value(&record(&[("name", text("Bob"))]))
        .await
        .unwrap();
    assert_eq!(auto_id, "1");
    let explicit = vista
        .insert_return_id_value(&record(&[("id", text("alice")), ("name", text("Alice"))]))
        .await
        .unwrap();
    assert_eq!(explicit, "alice");
    assert_eq!(vista.count().await.unwrap(), 2);
}

#[tokio::test]
async fn mock_shell_meets_the_dataset_write_contract() {
    use vantage_dataset::contract::{self, Fixture};
    for check in 0..5 {
        let shell = MockShell::new()
            .with_metadata(
                VistaMetadata::new()
                    .with_column(Column::new("id", "String").with_flag(ColumnFlag::Id))
                    .with_column(Column::new("name", "String"))
                    .with_column(Column::new("parent", "String"))
                    .with_id_column("id"),
            )
            .with_record(
                "in1",
                record(&[("name", text("a")), ("parent", text("p1"))]),
            )
            .with_record(
                "out1",
                record(&[("name", text("b")), ("parent", text("p2"))]),
            );
        let all = Vista::new("t", shell);
        let set = all
            .clone()
            .with_condition("parent", FilterOp::Eq, "p1")
            .unwrap();
        let f = Fixture {
            all: &all,
            set: &set,
            id: |s| s.to_string(),
            text,
            detects_outside: true,
        };
        match check {
            0 => contract::check_delete(&f).await,
            1 => contract::check_insert(&f).await,
            2 => contract::check_patch(&f).await,
            3 => contract::check_replace(&f).await,
            _ => contract::check_delete_all(&f).await,
        }
    }
}

#[tokio::test]
async fn narrowed_vista_refuses_writes_its_shell_cannot_confine() {
    let caps: VistaCapabilities = [Capability::Insert, Capability::Update, Capability::Delete]
        .into_iter()
        .collect();
    let v = Vista::new("t", MockShell::new().with_capabilities(caps))
        .with_condition("a", FilterOp::Eq, "x")
        .unwrap();
    assert!(v.is_narrowed());
    assert!(v.delete("1").await.unwrap_err().is_unsupported());
    assert!(v.delete_all().await.unwrap_err().is_unsupported());
}

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
