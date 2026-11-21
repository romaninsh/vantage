//! The NOTIFY opt-ins: `with_notify` for a vista's own table and the spec's
//! `postgres.listen` for others make a vista advertise `Subscribe`, and
//! reference traversal carries `with_notify` to the targets.

use std::error::Error;
use std::time::Duration;

use vantage_sql::postgres::PostgresDB;
use vantage_sql::postgres::vista::PostgresVistaSpec;
use vantage_vista::{Capability, Vista, VistaChange, VistaFactory};

use super::{TestResult, alice_row, clients_orders, product_table, setup, spec_resolver};

/// Open a fresh watch on `vista` and wait for its first change while
/// NOTIFYing `channel` every 50 ms: the stream LISTENs on first poll, so a
/// single NOTIFY could land before it does.
async fn invalidated_by(db: &PostgresDB, vista: &Vista, channel: &str) -> TestResult {
    let mut stream = vista.watch().await?;
    let next = std::future::poll_fn(|cx| stream.as_mut().poll_next(cx));
    let notify = async {
        loop {
            let sent = sqlx::query("SELECT pg_notify($1, '')")
                .bind(channel)
                .execute(db.pool())
                .await;
            if let Err(failed) = sent {
                return failed;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    };
    let change = tokio::time::timeout(Duration::from_secs(3), async {
        tokio::select! {
            change = next => Ok(change),
            failed = notify => Err(failed),
        }
    })
    .await
    .map_err(|_| format!("no change within 3 s after NOTIFY {channel}"))?;
    match change {
        Ok(Some(change)) => assert!(
            matches!(change?, VistaChange::Invalidated),
            "NOTIFY {channel} should invalidate"
        ),
        Ok(None) => panic!("watch stream ended"),
        Err(failed) => return Err(failed.into()),
    }
    Ok(())
}

/// A query-sourced vista is read-only and so never watches its own table;
/// `listen:` names the tables it reads, and is all it takes to subscribe.
#[cfg(feature = "rhai")]
#[tokio::test]
async fn query_vista_subscribes_to_listed_tables() -> TestResult {
    let (db, name) = setup("listen_query").await;

    for listen in [true, false] {
        let listen_line = if listen {
            format!("  listen: [{name}]\n")
        } else {
            String::new()
        };
        let vista = db.vista_factory().from_yaml(&format!(
            r#"
name: expensive_products
columns:
  id: {{ type: string, flags: [id] }}
  name: {{ type: string }}
postgres:
{listen_line}  rhai: |
    sql::select().from("{name}").field("id").field("name").where(sql::expr("price > 15"))
"#
        ))?;
        let caps = vista.capabilities();
        assert_eq!(caps.has(Capability::Subscribe), listen);
        assert!(!caps.has(Capability::Insert), "still read-only");
        if listen {
            invalidated_by(&db, &vista, &format!("{name}_changed")).await?;
        }
    }
    Ok(())
}

/// A writable vista with `with_notify` watches its own table and, through
/// `listen:`, the tables its columns reach. A fresh watch per channel, so
/// each NOTIFY proves its own channel is listened on.
#[tokio::test]
async fn writable_vista_listens_on_own_and_listed_tables() -> TestResult {
    let (db, client_t, orders_t) = clients_orders("listen_writable").await;
    let spec: PostgresVistaSpec = serde_yaml_ng::from_str(&format!(
        r#"
name: client
columns:
  id: {{ type: string, flags: [id] }}
  name: {{ type: string, flags: [title] }}
postgres:
  table: {client_t}
  listen: [{orders_t}, {client_t}]
"#
    ))?;
    let vista = db.vista_factory().with_notify(true).build_from_spec(spec)?;
    assert!(vista.capabilities().has(Capability::Subscribe));
    assert!(vista.capabilities().has(Capability::Insert));

    for table in [&client_t, &orders_t] {
        invalidated_by(&db, &vista, &format!("{table}_changed")).await?;
    }
    Ok(())
}

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
/// target on a triggered database comes back silently unwatchable. The
/// parent subscribing through `listen:` alone declares nothing about the
/// targets' triggers, so they stay unwatchable.
#[tokio::test]
async fn notify_opt_in_survives_reference_traversal() -> TestResult {
    let (db, client_t, orders_t) = clients_orders("refs_notify").await;
    let (client_spec, resolver) = clients_orders_specs(&client_t, &orders_t)?;

    for (notify, listen) in [(true, false), (false, false), (false, true)] {
        let mut spec = client_spec.clone();
        if listen {
            spec.driver
                .postgres
                .as_mut()
                .expect("postgres block")
                .listen = vec![client_t.clone()];
        }
        let clients = db
            .vista_factory()
            .with_resolver(resolver.clone())
            .with_notify(notify)
            .build_from_spec(spec)?;
        assert_eq!(
            clients.capabilities().has(Capability::Subscribe),
            notify || listen
        );

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
