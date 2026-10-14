//! The suite's client/orders cases: YAML references resolved through a spec
//! resolver, and `base:` derivations that inherit, transform and aggregate.
//! `vista_tests!` invokes it with the spec type and YAML driver key.

macro_rules! vista_client_orders_tests {
    ($spec:ty, $block:literal) => {
        fn spec_resolver(
            specs: Vec<(&str, $spec)>,
        ) -> std::sync::Arc<dyn vantage_vista::SpecResolver<$spec>> {
            let map: indexmap::IndexMap<String, $spec> = specs
                .into_iter()
                .map(|(name, spec)| (name.to_string(), spec))
                .collect();
            std::sync::Arc::new(move |name: &str| map.get(name).cloned())
        }

        /// `alice` out of a built `client` vista, for traversing her orders.
        async fn alice_row(
            clients: &vantage_vista::Vista,
        ) -> std::result::Result<vantage_types::Record<ciborium::Value>, Box<dyn std::error::Error>>
        {
            let (_id, alice) = clients
                .clone()
                .with_id(ciborium::Value::Text("alice".into()))?
                .get_some_value()
                .await?
                .expect("alice exists");
            Ok(alice)
        }

        /// YAML-declared `references:` lower to `with_one`/`with_many`, with the
        /// target table resolved by name through a spec resolver attached to
        /// the factory.
        #[tokio::test]
        async fn vista_yaml_references_resolve_via_resolver() -> TestResult {
            let (db, client_t, orders_t) = clients_orders("refs").await;
            let block = $block;

            let client_spec: $spec = serde_yaml_ng::from_str(&format!(
                r#"
name: client
columns:
  id: {{ type: string, flags: [id] }}
  name: {{ type: string, flags: [title] }}
{block}:
  table: {client_t}
references:
  orders:
    table: orders
    kind: has_many
    foreign_key: client_id
"#
            ))?;
            let orders_spec: $spec = serde_yaml_ng::from_str(&format!(
                r#"
name: orders
columns:
  id: {{ type: string, flags: [id] }}
  client_id: {{ type: string }}
  total: {{ type: int }}
{block}:
  table: {orders_t}
"#
            ))?;

            let factory = db
                .vista_factory()
                .with_resolver(spec_resolver(vec![("orders", orders_spec)]));
            let clients = factory.build_from_spec(client_spec)?;

            // Reference metadata surfaces from the wired relation.
            let refs = clients.references();
            assert_eq!(refs.len(), 1);
            assert_eq!(refs["orders"].kind, vantage_vista::ReferenceKind::HasMany);

            // Traversal resolves `orders` via the resolver and filters to
            // alice's rows.
            let alice = alice_row(&clients).await?;
            let alice_orders = clients.get_ref("orders", Some(&alice))?;
            assert!(alice_orders.is_narrowed());
            let rows = alice_orders.list_values().await?;
            assert_eq!(rows.len(), 2, "alice has 2 orders");
            assert!(rows.contains_key("o1"));
            assert!(rows.contains_key("o2"));
            assert!(!rows.contains_key("o3"));
            Ok(())
        }

        /// A derived vista: `base: client` resolves eagerly through the
        /// resolver, the base table's `select()` is seeded into the Rhai engine
        /// as `base` (transform mode), and the listed columns are inherited.
        /// Query source → read-only.
        #[cfg(feature = "rhai")]
        #[tokio::test]
        async fn vista_yaml_base_inherits_columns_and_transforms() -> TestResult {
            use vantage_vista::Capability;

            let (db, client_t, _orders_t) = clients_orders("base").await;
            let block = $block;

            let client_spec: $spec = serde_yaml_ng::from_str(&format!(
                r#"
name: client
columns:
  id: {{ type: string, flags: [id] }}
  name: {{ type: string, flags: [title] }}
{block}:
  table: {client_t}
"#
            ))?;
            let vip_spec: $spec = serde_yaml_ng::from_str(&format!(
                r#"
name: vip_clients
columns: {{}}
{block}:
  base: client
  inherit:
    columns: [id, name]
  rhai: |
    base.where(sql::expr("name = 'Alice'"))
"#
            ))?;

            let vista = db
                .vista_factory()
                .with_resolver(spec_resolver(vec![("client", client_spec)]))
                .build_from_spec(vip_spec)?;

            let caps = vista.capabilities();
            assert!(
                !caps.has(Capability::Insert),
                "derived (query-sourced) vista is read-only"
            );
            assert!(!caps.has(Capability::Update));
            assert!(!caps.has(Capability::Delete));

            // Transform applied (only Alice) and inherited columns surface.
            let rows = vista.list_values().await?;
            assert_eq!(rows.len(), 1, "rhai transform filtered to Alice");
            let alice = rows.get("alice").expect("alice present");
            assert_eq!(
                alice.get("name"),
                Some(&ciborium::Value::Text("Alice".into()))
            );
            Ok(())
        }

        /// The headline "derived aggregate" case: a `debtors`-style vista
        /// derived from `orders`, grouping by client and summing totals into a
        /// declared `total_due` column, with the grouping key inherited and
        /// re-keyed via `id_column`.
        #[cfg(feature = "rhai")]
        #[tokio::test]
        async fn vista_yaml_base_aggregates_into_declared_column() -> TestResult {
            let (db, _client_t, orders_t) = clients_orders("agg").await;
            let block = $block;

            let orders_spec: $spec = serde_yaml_ng::from_str(&format!(
                r#"
name: orders
columns:
  id: {{ type: string, flags: [id] }}
  client_id: {{ type: string }}
  total: {{ type: int }}
{block}:
  table: {orders_t}
"#
            ))?;
            let totals_spec: $spec = serde_yaml_ng::from_str(&format!(
                r#"
name: client_totals
id_column: client_id
columns:
  total_due: {{ type: int }}
{block}:
  base: orders
  inherit:
    columns: [client_id]
  rhai: |
    base.clear_fields().field("client_id").expression(sql::expr("SUM(total) AS total_due")).group_by(sql::expr("client_id"))
"#
            ))?;

            let vista = db
                .vista_factory()
                .with_resolver(spec_resolver(vec![("orders", orders_spec)]))
                .build_from_spec(totals_spec)?;

            // Re-keyed by client_id; one row per client with the summed total.
            let rows = vista.list_values().await?;
            assert_eq!(rows.len(), 2, "one row per client");
            assert_eq!(
                numeric(rows.get("alice").unwrap().get("total_due")),
                Some(30)
            );
            assert_eq!(numeric(rows.get("bob").unwrap().get("total_due")), Some(30));
            Ok(())
        }

        /// An integer out of a CBOR scalar, whether the backend returned an
        /// integer (SQLite) or a decimal: Postgres and MySQL `SUM(bigint)`
        /// return `numeric`/`DECIMAL`, surfacing as `Tag(_, Text("30"))`.
        #[cfg(feature = "rhai")]
        fn numeric(v: Option<&ciborium::Value>) -> Option<i64> {
            match v? {
                ciborium::Value::Integer(i) => i64::try_from(*i).ok(),
                ciborium::Value::Text(s) => s.parse().ok(),
                ciborium::Value::Tag(_, inner) => match inner.as_ref() {
                    ciborium::Value::Text(s) => s.parse().ok(),
                    _ => None,
                },
                _ => None,
            }
        }
    };
}
