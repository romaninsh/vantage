//! Relations of contained records (a line's `product`), served through the
//! resolver handed to `build_contained_vista`.

use std::sync::Arc;

use ciborium::Value as CborValue;
use vantage_core::error;
use vantage_dataset::traits::ReadableValueSet;
use vantage_vista::{
    Capability, Catalog, Column, ColumnFlag, ContainedWriteback, JoinKey, Reference, Vista,
    VistaMetadata, VistaResolver, build_contained_vista, mocks::MockShell,
};

use super::{line, rec, t};

fn products() -> MockShell {
    let metadata = VistaMetadata::new()
        .with_column(Column::new("id", "String").with_flag(ColumnFlag::Id))
        .with_id_column("id");
    MockShell::new()
        .with_metadata(metadata)
        .with_record("flux", rec(&[("id", t("flux"))]))
        .with_record("delorean", rec(&[("id", t("delorean"))]))
}

/// Bare targets by target name; only `products` is known.
fn resolver() -> Arc<dyn VistaResolver> {
    Arc::new(|target: &str| match target {
        "products" => Ok(Vista::new("products", products())),
        _ => Err(error!("no such model", model = target).mark_not_found()),
    })
}

fn discard() -> ContainedWriteback {
    Arc::new(|_| Box::pin(async { Ok(()) }))
}

/// Two order lines whose `product` relation joins to the `products` model.
fn lines(resolver: Option<Arc<dyn VistaResolver>>) -> Vista {
    let schema = VistaMetadata::new()
        .with_column(Column::new("product", "String"))
        .with_column(Column::new("quantity", "i64"))
        .with_reference(Reference::has_one(
            "product",
            "products",
            JoinKey::new("product", "id"),
        ));
    let host = CborValue::Array(vec![line("flux", 3, 120), line("delorean", 1, 135)]);
    let reference = Reference::contains_many("lines", "lines");
    build_contained_vista(&reference, schema, Some(&host), Some(discard()), resolver).unwrap()
}

/// The resolver is keyed by target, so a model catalog serves record
/// relations as it is.
#[tokio::test]
async fn a_catalog_resolves_record_relations_by_target() {
    let catalog = Catalog::new().with("products", |name: &str| Ok(Vista::new(name, products())));
    let lines = lines(Some(Arc::new(catalog)));
    let first = lines.get_value("0").await.unwrap().unwrap();

    let product = lines.get_ref("product", Some(&first)).unwrap();
    assert_eq!(product.name(), "products");
    let rows = product.list_values().await.unwrap();
    assert_eq!(rows.keys().collect::<Vec<_>>(), ["flux"]);
}

/// Without a row, the relation is the bare target.
#[tokio::test]
async fn a_record_relation_without_a_row_is_the_bare_target() {
    let product = lines(Some(resolver())).get_ref("product", None).unwrap();
    assert!(!product.is_narrowed());
    assert_eq!(product.list_values().await.unwrap().len(), 2);
}

#[test]
fn an_undeclared_record_relation_is_not_found() {
    let err = lines(Some(resolver()))
        .get_ref("supplier", None)
        .unwrap_err();
    assert!(err.is_not_found(), "{err:?}");
}

#[test]
fn without_a_resolver_record_relations_are_refused() {
    let lines = lines(None);
    assert!(!lines.capabilities().has(Capability::Traverse));
    let err = lines.get_ref("product", None).unwrap_err();
    assert!(err.is_unsupported(), "{err:?}");
}
