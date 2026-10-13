use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use ciborium::Value as CborValue;
use vantage_core::{Result, error};
use vantage_dataset::traits::ReadableValueSet;
use vantage_types::Record;

use super::*;
use crate::{Column, ColumnFlag, JoinKey, Reference, VistaMetadata, mocks::MockShell};

fn text(s: &str) -> CborValue {
    CborValue::Text(s.into())
}

fn record(pairs: &[(&str, &str)]) -> Record<CborValue> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), text(v)))
        .collect()
}

/// One crew member of launch `L1`.
fn crew(name: &str) -> Result<Vista> {
    let metadata = VistaMetadata::new()
        .with_column(Column::new("id", "String").with_flag(ColumnFlag::Id))
        .with_column(Column::new("launch_id", "String"))
        .with_id_column("id");
    let shell = MockShell::new()
        .with_metadata(metadata)
        .with_record("c1", record(&[("id", "c1"), ("launch_id", "L1")]))
        .with_record("c2", record(&[("id", "c2"), ("launch_id", "L2")]));
    Ok(Vista::new(name, shell))
}

fn launch_crew() -> Reference {
    Reference::has_many("crew", "launch_crew", JoinKey::new("id", "launch_id"))
}

#[tokio::test]
async fn traverses_a_reference_through_the_catalog() {
    let catalog = Catalog::new().with("launch_crew", crew);
    let reference = launch_crew();
    let target = catalog.vista(&reference.target).unwrap();
    let narrowed = reference.narrow(target, &record(&[("id", "L1")])).unwrap();
    let rows = narrowed.list_values().await.unwrap();
    assert_eq!(rows.keys().collect::<Vec<_>>(), ["c1"]);
}

/// A model the catalog doesn't hold is not-found; a registered model whose
/// loader fails (its backend unreachable) is not, and keeps its message, so
/// a caller can tell "no such relation" from "can't load it right now".
#[test]
fn a_missing_model_is_not_found_and_an_unreachable_one_is_not() {
    let up = Arc::new(AtomicBool::new(true));
    let source_up = up.clone();
    let catalog = Catalog::new().with("launch_crew", move |name: &str| {
        if !source_up.load(Ordering::SeqCst) {
            return Err(error!("crew source unavailable", status = 503));
        }
        crew(name)
    });

    assert!(catalog.vista("launch_crew").is_ok());

    up.store(false, Ordering::SeqCst);
    let unreachable = catalog.vista("launch_crew").unwrap_err();
    assert!(!unreachable.is_not_found(), "{unreachable:?}");
    assert!(
        unreachable.to_string().contains("crew source unavailable"),
        "{unreachable}"
    );

    let missing = catalog.vista("does_not_exist").unwrap_err();
    assert!(missing.is_not_found(), "{missing:?}");
    assert_eq!(
        missing.context.get("model").map(String::as_str),
        Some("\"does_not_exist\"")
    );
}

/// A loader's not-found error stays not-found too: the catalog adds no kind
/// of its own.
#[test]
fn a_loader_error_keeps_its_kind() {
    let catalog = Catalog::new().with("gone", |_: &str| -> Result<Vista> {
        Err(error!("table dropped").mark_not_found())
    });
    assert!(catalog.vista("gone").unwrap_err().is_not_found());
}

#[test]
fn the_loader_is_called_with_the_model_name() {
    let catalog = Catalog::new()
        .with("a", |name: &str| crew(name))
        .with("b", |name: &str| crew(name));
    assert_eq!(catalog.vista("a").unwrap().name(), "a");
    assert_eq!(catalog.vista("b").unwrap().name(), "b");
}

#[test]
fn with_replaces_an_earlier_loader() {
    let catalog = Catalog::new()
        .with("m", |_: &str| crew("first"))
        .with("m", |_: &str| crew("second"));
    assert_eq!(catalog.vista("m").unwrap().name(), "second");
    assert_eq!(format!("{catalog:?}"), r#"Catalog { models: ["m"] }"#);
}

/// A catalog goes wherever a resolver is expected, and a clone shares its
/// loaders.
#[test]
fn a_catalog_is_a_resolver() {
    let catalog = Catalog::new().with("launch_crew", crew);
    let resolver: Arc<dyn VistaResolver> = Arc::new(catalog.clone());
    assert_eq!(resolver.vista("launch_crew").unwrap().name(), "launch_crew");
    assert!(resolver.vista("nope").unwrap_err().is_not_found());
}
