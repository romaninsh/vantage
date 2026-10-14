use vantage_dataset::{InsertableValueSet, ReadableValueSet, WritableValueSet};

use super::{MockShell, record, text, user_vista};
use crate::{Capability, Column, ColumnFlag, FilterOp, Vista, VistaCapabilities, VistaMetadata};

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
