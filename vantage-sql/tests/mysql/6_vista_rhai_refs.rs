//! Scripted references: a YAML reference with a `mysql: { rhai }` block
//! builds its traversal target by evaluating the script, with `table(name)`
//! resolved through the factory's spec resolver and the parent row exposed as
//! `row` and `me`.
//!
//! Requires a running MySQL on `mysql://vantage:vantage@localhost:3306/vantage`.

use std::error::Error;

use vantage_dataset::prelude::*;
use vantage_vista::{Capability, VistaFactory};

use fixture::{Fixture, ORDERS_SCRIPT, row_with_id, sorted_ids};

#[path = "6_vista_rhai_refs/fixture.rs"]
mod fixture;

type TestResult = std::result::Result<(), Box<dyn Error>>;

#[tokio::test]
async fn build_script_reference_resolves_target() -> TestResult {
    let f = Fixture::new("resolve").await;
    let factory = f.db.vista_factory().with_resolver(f.resolver());
    let clients = factory.build_from_spec(f.client_spec(ORDERS_SCRIPT))?;
    let reference = clients.reference("orders").expect("orders ref");
    assert!(reference.build_script.is_some());

    let alice = clients.get_value("alice").await?.expect("alice exists");
    let orders = clients.get_ref("orders", Some(&alice))?;
    assert_eq!(orders.name(), "orders");
    let rows = orders.list_values().await?;
    assert_eq!(sorted_ids(&rows), vec!["o2"]);

    f.drop_tables().await;
    Ok(())
}

/// The metadata names a scripted reference's target by its spec (`orders`),
/// not by the physical table that spec reads, and carries the script.
#[tokio::test]
async fn build_script_reference_targets_the_spec_name() -> TestResult {
    let f = Fixture::new("target").await;
    let factory = f.db.vista_factory().with_resolver(f.resolver());
    let clients = factory.build_from_spec(f.client_spec(ORDERS_SCRIPT))?;
    let reference = clients.reference("orders").expect("orders ref");
    assert_eq!(reference.target, "orders");
    assert_ne!(reference.target, f.orders_t);
    assert_eq!(
        reference.build_script.as_deref().map(str::trim),
        Some(ORDERS_SCRIPT)
    );

    f.drop_tables().await;
    Ok(())
}

#[tokio::test]
async fn build_script_reference_sees_row() -> TestResult {
    let f = Fixture::new("row").await;
    let factory = f.db.vista_factory().with_resolver(f.resolver());
    let clients = factory.build_from_spec(
        f.client_spec(r#"table("orders").where("client_id", row.id).where("total", "lt", 15)"#),
    )?;

    let orders = clients.get_ref("orders", Some(&row_with_id("alice")))?;
    assert_eq!(sorted_ids(&orders.list_values().await?), vec!["o1"]);
    // The target is the writable table.
    assert!(orders.capabilities().has(Capability::Insert));

    f.drop_tables().await;
    Ok(())
}

/// A scripted reference is still a traversal: both the table and the
/// query-sourced view advertise `Traverse`.
#[tokio::test]
async fn capabilities_advertise_traverse_for_scripted_refs() -> TestResult {
    let f = Fixture::new("caps").await;
    let factory = f.db.vista_factory().with_resolver(f.resolver());
    let orders = factory.build_from_spec(f.orders_spec())?;
    assert!(orders.capabilities().has(Capability::Traverse));
    let view = factory.build_from_spec(f.client_view_spec())?;
    assert!(view.capabilities().has(Capability::Traverse));

    f.drop_tables().await;
    Ok(())
}

#[tokio::test]
async fn scripted_ref_from_query_sourced_view_row() -> TestResult {
    let f = Fixture::new("view").await;
    let factory = f.db.vista_factory().with_resolver(f.resolver());
    let view = factory.build_from_spec(f.client_view_spec())?;
    assert!(
        !view.capabilities().has(Capability::Insert),
        "query-sourced view is read-only"
    );

    let alice = view.get_value("alice").await?.expect("alice in view");
    let orders = view.get_ref("orders", Some(&alice))?;
    assert_eq!(sorted_ids(&orders.list_values().await?), vec!["o2"]);

    f.drop_tables().await;
    Ok(())
}

#[tokio::test]
async fn scripted_ref_declared_on_base_derived_vista() -> TestResult {
    let f = Fixture::new("derived").await;
    let factory = f.db.vista_factory().with_resolver(f.resolver());
    let derived = factory.build_from_spec(f.derived_client_spec())?;
    let reference = derived.reference("orders").expect("orders ref");
    assert!(reference.build_script.is_some());

    let alice = derived.get_value("alice").await?.expect("alice in view");
    let orders = derived.get_ref("orders", Some(&alice))?;
    assert_eq!(sorted_ids(&orders.list_values().await?), vec!["o2"]);

    f.drop_tables().await;
    Ok(())
}

#[tokio::test]
async fn script_table_unknown_to_resolver_errors() -> TestResult {
    let f = Fixture::new("unknown").await;
    let factory = f.db.vista_factory().with_resolver(f.resolver());
    let clients = factory.build_from_spec(f.client_spec(r#"table("nope")"#))?;
    let err = clients
        .get_ref("orders", Some(&row_with_id("alice")))
        .map(drop)
        .expect_err("an unknown table must fail");
    let msg = err.to_string();
    assert!(
        msg.contains("unknown table") && msg.contains("nope"),
        "{msg}"
    );

    f.drop_tables().await;
    Ok(())
}

#[tokio::test]
async fn script_table_without_resolver_errors() -> TestResult {
    let f = Fixture::new("noresolver").await;
    let factory = f.db.vista_factory();
    let clients = factory.build_from_spec(f.client_spec(ORDERS_SCRIPT))?;
    let err = clients
        .get_ref("orders", Some(&row_with_id("alice")))
        .map(drop)
        .expect_err("table() without a spec resolver must fail");
    assert!(err.to_string().contains("spec resolver"), "{err}");

    f.drop_tables().await;
    Ok(())
}
