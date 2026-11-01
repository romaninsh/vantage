use vantage_core::VantageError;
use vantage_dataset::traits::{ReadableValueSet, WritableValueSet};
use vantage_vista::mocks::MockShell;
use vantage_vista::{Capability, Denials, Vista};

use super::{rec, t};

fn pii() -> Denials {
    let mut d = Denials::new();
    d.deny(Capability::Delete, "policy/pii.yaml#no-delete");
    d.deny_column("ssn", "policy/pii.yaml#pii-read-only");
    d
}

fn account(denials: Denials) -> Vista {
    let shell = MockShell::new()
        .with_record(
            "1",
            rec(&[("id", t("1")), ("name", t("Ann")), ("ssn", t("123"))]),
        )
        .with_denials(denials);
    Vista::new("account", shell)
}

fn rule(err: &VantageError) -> Option<&str> {
    err.context.get("rule").map(String::as_str)
}

#[tokio::test]
async fn a_denied_capability_is_forbidden_and_named() {
    let vista = account(pii());
    assert!(!vista.capabilities().has(Capability::Delete));
    let err = vista.delete("1".to_string()).await.unwrap_err();
    assert!(err.is_forbidden(), "{err:?}");
    assert_eq!(rule(&err), Some("policy/pii.yaml#no-delete"));
    assert!(vista.get_value("1").await.unwrap().is_some());
}

#[tokio::test]
async fn patch_of_a_denied_column_is_forbidden_replace_keeps_it() {
    let vista = account(pii());
    let err = vista
        .patch_value("1".to_string(), &rec(&[("ssn", t("x"))]))
        .await
        .unwrap_err();
    assert!(err.is_forbidden(), "{err:?}");
    assert_eq!(rule(&err), Some("policy/pii.yaml#pii-read-only"));

    let stored = vista
        .replace_value(
            "1".to_string(),
            &rec(&[("id", t("1")), ("name", t("a")), ("ssn", t("x"))]),
        )
        .await
        .unwrap();
    assert_eq!(stored.get("name"), Some(&t("a")));
    assert_eq!(stored.get("ssn"), Some(&t("123")));

    // A record that leaves the column out doesn't erase it either.
    vista
        .upsert_value("1".to_string(), &rec(&[("id", t("1")), ("name", t("b"))]))
        .await
        .unwrap();
    let row = vista.get_value("1").await.unwrap().unwrap();
    assert_eq!(row.get("ssn"), Some(&t("123")));
}

#[tokio::test]
async fn with_insert_denied_replace_and_upsert_create_no_row() {
    let mut denials = Denials::new();
    denials.deny(Capability::Insert, "policy/no-new.yaml");
    let vista = account(denials);

    vista
        .replace_value("1".to_string(), &rec(&[("id", t("1")), ("name", t("a"))]))
        .await
        .unwrap();
    assert_eq!(
        vista.get_value("1").await.unwrap().unwrap().get("name"),
        Some(&t("a"))
    );

    let new = rec(&[("id", t("2")), ("name", t("Bob"))]);
    let err = vista
        .replace_value("2".to_string(), &new)
        .await
        .unwrap_err();
    assert!(err.is_forbidden(), "{err:?}");
    assert_eq!(rule(&err), Some("policy/no-new.yaml"));
    let err = vista.upsert_value("2".to_string(), &new).await.unwrap_err();
    assert!(err.is_forbidden(), "{err:?}");
    assert_eq!(rule(&err), Some("policy/no-new.yaml"));
    assert!(vista.get_value("2").await.unwrap().is_none());
}

#[tokio::test]
async fn deny_survives_clone_and_cannot_be_undone() {
    let vista = account(Denials::new()).deny(pii());
    let clone = vista.clone().deny(Denials::new());
    assert_eq!(clone.denials(), pii());
    let err = clone.delete("1".to_string()).await.unwrap_err();
    assert!(err.is_forbidden(), "{err:?}");
}
