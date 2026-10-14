//! The `with_notify` opt-in: it alone makes a vista advertise `Subscribe`,
//! and reference traversal carries it to the targets.

use std::error::Error;

use vantage_sql::postgres::vista::PostgresVistaSpec;
use vantage_vista::{Capability, VistaFactory};

use super::{TestResult, alice_row, clients_orders, product_table, setup, spec_resolver};

#[tokio::test]
async fn vista_is_watchable_only_when_notify_is_declared() -> TestResult {
    let (db, name) = setup("caps_notify").await;

    // Opting in is what makes the LISTEN/NOTIFY feed advertisable — the
    // application is the only party that knows the trigger exists.
    for notify in [true, false] {
        let vista = db
            .vista_factory()
            .with_notify(notify)
            .from_table(product_table(db.clone(), &name))?;
        assert_eq!(vista.capabilities().has(Capability::Subscribe), notify);
    }
    Ok(())
}

/// The `client` spec (with a `has_many` to `orders`) plus a resolver that can
/// find the `orders` target by name.
#[allow(clippy::type_complexity)]
fn clients_orders_specs(
    client_t: &str,
    orders_t: &str,
) -> std::result::Result<
    (
        PostgresVistaSpec,
        std::sync::Arc<dyn vantage_vista::SpecResolver<PostgresVistaSpec>>,
    ),
    Box<dyn Error>,
> {
    let client_spec: PostgresVistaSpec = serde_yaml_ng::from_str(&format!(
        r#"
name: client
columns:
  id: {{ type: string, flags: [id] }}
  name: {{ type: string, flags: [title] }}
postgres:
  table: {client_t}
references:
  orders:
    table: orders
    kind: has_many
    foreign_key: client_id
"#
    ))?;
    let orders_spec: PostgresVistaSpec = serde_yaml_ng::from_str(&format!(
        r#"
name: orders
columns:
  id: {{ type: string, flags: [id] }}
  client_id: {{ type: string }}
  total: {{ type: int }}
postgres:
  table: {orders_t}
"#
    ))?;
    Ok((client_spec, spec_resolver(vec![("orders", orders_spec)])))
}

/// Traversing a reference builds a fresh factory internally, which must carry
/// the parent's `with_notify` declaration across — otherwise every relation
/// target on a triggered database comes back silently unwatchable.
#[tokio::test]
async fn notify_opt_in_survives_reference_traversal() -> TestResult {
    let (db, client_t, orders_t) = clients_orders("refs_notify").await;
    let (client_spec, resolver) = clients_orders_specs(&client_t, &orders_t)?;

    for notify in [true, false] {
        let clients = db
            .vista_factory()
            .with_resolver(resolver.clone())
            .with_notify(notify)
            .build_from_spec(client_spec.clone())?;
        assert_eq!(clients.capabilities().has(Capability::Subscribe), notify);

        let alice = alice_row(&clients).await?;
        let orders = clients.get_ref("orders", Some(&alice))?;
        assert_eq!(
            orders.capabilities().has(Capability::Subscribe),
            notify,
            "a target narrowed to a row should inherit the parent's notify opt-in"
        );

        let target = clients.get_ref("orders", None)?;
        assert_eq!(
            target.capabilities().has(Capability::Subscribe),
            notify,
            "the bare target should inherit the parent's notify opt-in"
        );
    }
    Ok(())
}
