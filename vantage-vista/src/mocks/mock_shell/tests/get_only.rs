use std::sync::Arc;
use std::time::Duration;

use tokio::sync::Semaphore;

use super::{text, three_users};
use crate::{Capability, FilterOp, TableShell};

#[tokio::test]
async fn a_get_only_shell_answers_get_and_refuses_listing() {
    let shell = three_users().get_only();
    let bob = shell.get("2").await.unwrap().unwrap();
    assert_eq!(bob.get("name"), Some(&text("Bob")));
    assert!(shell.get("9").await.unwrap().is_none());

    let refusals = [
        shell.list().await.err(),
        shell.count().await.err(),
        shell.fetch_window(0, 10).await.err(),
    ];
    for err in refusals {
        let err = err.expect("a get-only shell lists nothing");
        assert!(err.is_unsupported(), "{err}");
    }
}

#[tokio::test]
async fn a_get_only_shell_advertises_nothing_and_refuses_search() {
    let mut shell = three_users().get_only();
    assert!(!shell.capabilities().has(Capability::Window));
    let err = shell.set_search(Some("bo")).unwrap_err();
    assert!(err.is_unsupported(), "{err}");
}

#[tokio::test]
async fn conditions_still_narrow_a_get_only_get() {
    let mut shell = three_users().get_only();
    let vip = crate::Condition::new("vip_flag", FilterOp::Eq, true);
    shell.add_condition(&vip).unwrap();
    assert!(shell.get("1").await.unwrap().is_some());
    assert!(shell.get("2").await.unwrap().is_none(), "Bob is not a VIP");
}

#[tokio::test]
async fn gets_are_logged_in_dispatch_order_across_clones() {
    let shell = three_users().get_only();
    let clone = shell.clone();
    shell.get("3").await.unwrap();
    clone.get("1").await.unwrap();
    assert_eq!(shell.get_log(), ["3", "1"]);
    assert_eq!(shell.max_concurrent_gets(), 1);

    let plain = three_users();
    plain.get("1").await.unwrap();
    assert!(plain.get_log().is_empty(), "a plain mock logs nothing");
}

#[tokio::test]
async fn a_gated_get_waits_for_a_permit() {
    let gate = Arc::new(Semaphore::new(0));
    let shell = three_users().get_only().gated(gate.clone());

    let (a, b) = (shell.clone(), shell.clone());
    let first = tokio::spawn(async move { a.get("1").await });
    let second = tokio::spawn(async move { b.get("2").await });
    tokio::time::sleep(Duration::from_millis(20)).await;
    assert!(!first.is_finished() && !second.is_finished());
    assert_eq!(shell.get_log().len(), 2, "both dispatched before the gate");
    assert_eq!(shell.max_concurrent_gets(), 2);

    gate.add_permits(2);
    assert!(first.await.unwrap().unwrap().is_some());
    assert!(second.await.unwrap().unwrap().is_some());
    assert_eq!(gate.available_permits(), 0, "each get consumed its permit");
}

#[tokio::test]
async fn a_closed_gate_is_an_error() {
    let gate = Arc::new(Semaphore::new(0));
    gate.close();
    let err = three_users().gated(gate).get("1").await.unwrap_err();
    assert!(err.to_string().contains("gate is closed"), "{err}");
}
