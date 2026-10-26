use std::sync::{Arc, Mutex};

use vantage_core::error;
use vantage_dataset::traits::ReadableValueSet;
use vantage_vista::mocks::MockShell;
use vantage_vista::{
    Catalog, Column, ColumnFlag, JoinKey, Reference, Vista, VistaMetadata, VistaResolver,
};

use super::{rec, t};

fn clients() -> Vista {
    let meta = VistaMetadata::new()
        .with_column(Column::new("id", "String").with_flag(ColumnFlag::Id))
        .with_id_column("id");
    let shell = MockShell::new()
        .with_metadata(meta)
        .with_record("c1", rec(&[("id", t("c1"))]))
        .with_record("c2", rec(&[("id", t("c2"))]));
    Vista::new("client", shell)
}

fn catalog() -> Catalog {
    Catalog::new().with("client", |name: &str| Ok(clients().with_name(name)))
}

#[test]
fn a_missing_model_is_not_found_and_an_unreachable_one_is_not() {
    let catalog = catalog().with("down", |_: &str| Err(error!("connection refused")));
    let missing = catalog.vista("nope").unwrap_err();
    assert!(missing.is_not_found(), "{missing:?}");
    let down = catalog.vista("down").unwrap_err();
    assert!(!down.is_not_found(), "{down:?}");
}

#[test]
fn a_loader_error_keeps_its_kind() {
    let catalog = Catalog::new().with("m", |_: &str| Err(error!("no access").mark_unsupported()));
    assert!(catalog.vista("m").unwrap_err().is_unsupported());
}

#[test]
fn the_loader_is_called_with_the_model_name_each_time() {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = seen.clone();
    let catalog = Catalog::new().with("m", move |name: &str| {
        log.lock().unwrap().push(name.to_string());
        Ok(clients())
    });
    catalog.vista("m").unwrap();
    catalog.vista("m").unwrap();
    assert_eq!(*seen.lock().unwrap(), ["m", "m"]);
}

#[test]
fn registering_a_model_again_replaces_its_loader_in_a_cloned_resolver() {
    let catalog = catalog().with("client", |name: &str| {
        Ok(clients().with_name(format!("{name}-v2")))
    });
    let resolver: Arc<dyn VistaResolver> = Arc::new(catalog.clone());
    assert_eq!(resolver.vista("client").unwrap().name(), "client-v2");
}

#[test]
fn a_catalog_debug_lists_its_model_names() {
    let text = format!("{:?}", catalog().with("order", |_: &str| Ok(clients())));
    assert!(text.contains("Catalog"), "{text}");
    assert!(text.contains("client") && text.contains("order"), "{text}");
}

#[test]
fn a_closure_is_a_resolver() {
    let resolver: Arc<dyn VistaResolver> = Arc::new(|name: &str| match name {
        "client" => Ok(clients()),
        _ => Err(error!("no such table", name = name).mark_not_found()),
    });
    assert_eq!(resolver.vista("client").unwrap().name(), "client");
    assert!(resolver.vista("nope").unwrap_err().is_not_found());
}

#[tokio::test]
async fn a_reference_traverses_through_the_catalog() {
    let reference = Reference::has_one("owner", "client", JoinKey::new("owner_id", "id"));
    let target = catalog().vista(&reference.target).unwrap();
    let narrowed = reference
        .narrow(target, &rec(&[("owner_id", t("c2"))]))
        .unwrap();
    let rows = narrowed.list_values().await.unwrap();
    assert_eq!(rows.keys().collect::<Vec<_>>(), ["c2"]);
}
