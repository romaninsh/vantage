use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use vantage_dataset::traits::ReadableValueSet;
use vantage_vista::mocks::MockShell;
use vantage_vista::rhai::{Handle, ResolverCell};
use vantage_vista::{Transaction, Vista, VistaResolver};

/// A resolver handing out mock Vistas, counting its calls.
fn counting() -> (Arc<dyn VistaResolver>, Arc<AtomicUsize>) {
    let calls = Arc::new(AtomicUsize::new(0));
    let seen = calls.clone();
    let resolver = Arc::new(move |name: &str| {
        seen.fetch_add(1, Ordering::SeqCst);
        Ok(Vista::new(name, MockShell::new()))
    });
    (resolver, calls)
}

#[test]
fn a_cell_resolves_through_its_base() {
    let (base, calls) = counting();
    let cell = ResolverCell::new(base);

    assert_eq!(cell.vista("a").unwrap().name(), "a");
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(!cell.is_pointed());
}

#[test]
fn pointing_redirects_until_the_guard_drops() {
    let (base, base_calls) = counting();
    let (over, over_calls) = counting();
    let cell = ResolverCell::new(base);

    let guard = cell.point(over);
    assert!(cell.is_pointed());
    cell.vista("a").unwrap();
    assert_eq!(over_calls.load(Ordering::SeqCst), 1);
    assert_eq!(base_calls.load(Ordering::SeqCst), 0);

    drop(guard);
    assert!(!cell.is_pointed());
    cell.vista("a").unwrap();
    assert_eq!(over_calls.load(Ordering::SeqCst), 1);
    assert_eq!(base_calls.load(Ordering::SeqCst), 1);
}

#[test]
fn a_handle_made_before_pointing_follows_the_cell() {
    let (base, base_calls) = counting();
    let (over, over_calls) = counting();
    let cell = ResolverCell::new(base);
    let handle = Handle::named("a").with_resolver(Arc::new(cell.clone()));

    let _guard = cell.point(over);
    handle.resolve(None).unwrap();

    assert_eq!(over_calls.load(Ordering::SeqCst), 1);
    assert_eq!(base_calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn a_vista_in_hand_is_adopted() {
    let (base, _) = counting();
    let tx = Transaction::new();
    // MockShell can't rebind, so only a wrapped Vista refuses to list.
    let vista = tx.resolver(base).adopt(Vista::new("a", MockShell::new()));

    let err = vista.list_values().await.unwrap_err();
    assert!(
        err.to_string().contains("can't take part in a transaction"),
        "{err}"
    );
}
