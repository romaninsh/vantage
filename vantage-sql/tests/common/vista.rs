//! The vista suite each SQL backend runs: typed and YAML-built vistas over a
//! seeded product table, and YAML references and `base:` derivations over a
//! client/orders pair.
//!
//! A backend's `vista` module imports `vantage_dataset::prelude::*` and
//! `vantage_vista::VistaFactory`, and defines `TestResult` and two fixtures, each
//! taking a `suffix` that keeps tests apart where the database is shared:
//!
//! - `async fn product(suffix: &str) -> (DB, Table<DB, EmptyEntity>)`: a
//!   product table with `id`, `name`, `price` and `is_deleted`, holding
//!   `a`/Alpha/10, `b`/Beta/20 (deleted) and `c`/Gamma/30;
//! - `async fn clients_orders(suffix: &str) -> (DB, String, String)`:
//!   clients `alice` and `bob`, orders `o1`, `o2` (alice, 10 and 20) and
//!   `o3` (bob, 30), returning the client and orders table names.
//!
//! It then invokes `vista_tests!(SpecType, "block")`, `block` being the
//! dialect's YAML driver key.

macro_rules! vista_tests {
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

        #[tokio::test]
        async fn vista_get_value_by_id() -> TestResult {
            let (db, table) = product("get").await;
            let vista = db.vista_factory().from_table(table)?;

            let row = vista.get_value("b").await?.expect("row b exists");
            assert_eq!(
                row.get("name"),
                Some(&ciborium::Value::Text("Beta".to_string()))
            );

            let missing = vista.get_value("nope").await?;
            assert!(missing.is_none());
            Ok(())
        }

        #[tokio::test]
        async fn vista_count_with_eq_condition() -> TestResult {
            let (db, table) = product("count").await;
            let mut vista = db.vista_factory().from_table(table)?;

            assert_eq!(vista.count().await?, 3);

            vista.add_condition(
                "is_deleted",
                vantage_vista::FilterOp::Eq,
                ciborium::Value::Bool(false),
            )?;
            assert_eq!(vista.count().await?, 2);

            let rows = vista.list_values().await?;
            assert_eq!(rows.len(), 2);
            assert!(rows.contains_key("a"));
            assert!(rows.contains_key("c"));
            assert!(!rows.contains_key("b"));
            Ok(())
        }

        /// SQL compares whole column values, so a path into one is refused as
        /// `Unimplemented`, and `with_query` hands it back as residual.
        #[tokio::test]
        async fn vista_condition_on_a_dotted_path_is_refused() -> TestResult {
            let (db, table) = product("dotted").await;
            let vista = db.vista_factory().from_table(table)?;
            crate::dotted::assert_dotted_path_is_refused(&vista, "name.first");
            Ok(())
        }

        #[tokio::test]
        async fn vista_yaml_loads_table_and_columns() -> TestResult {
            let (db, table) = product("yaml").await;
            let (block, name) = ($block, table.table_name());

            let yaml = format!(
                r#"
name: product_view
columns:
  id:
    type: string
    flags: [id]
  name:
    type: string
    flags: [title, searchable]
  price:
    type: int
{block}:
  table: {name}
"#
            );

            let vista = db.vista_factory().from_yaml(&yaml)?;

            assert_eq!(vista.name(), "product_view");
            assert_eq!(vista.id_column(), Some("id"));
            assert_eq!(vista.title_columns(), vec!["name"]);

            let rows = vista.list_values().await?;
            assert_eq!(rows.len(), 3);
            assert!(rows.contains_key("a"));
            Ok(())
        }

        #[tokio::test]
        async fn vista_writes_round_trip_via_cbor() -> TestResult {
            let (db, table) = product("write").await;
            let vista = db.vista_factory().from_table(table)?;

            let record: vantage_types::Record<ciborium::Value> = vec![
                ("name".to_string(), ciborium::Value::Text("Delta".into())),
                ("price".to_string(), ciborium::Value::Integer(99i64.into())),
                ("is_deleted".to_string(), ciborium::Value::Bool(false)),
            ]
            .into_iter()
            .collect();

            vista.insert_value("d", &record).await?;

            let fetched = vista.get_value("d").await?.expect("inserted");
            assert_eq!(
                fetched.get("name"),
                Some(&ciborium::Value::Text("Delta".into()))
            );

            vista.delete("d").await?;
            assert!(vista.get_value("d").await?.is_none());
            Ok(())
        }

        /// A vista whose source is a Rhai-built SELECT (not a physical table).
        /// The script filters `price > 15`, so only two products surface, and
        /// the vista is read-only.
        #[cfg(feature = "rhai")]
        #[tokio::test]
        async fn vista_yaml_rhai_source_is_read_only_and_filters() -> TestResult {
            use vantage_vista::Capability;

            let (db, table) = product("rhai").await;
            let (block, name) = ($block, table.table_name());

            let yaml = format!(
                r#"
name: expensive_products
columns:
  id:
    type: string
    flags: [id]
  name:
    type: string
{block}:
  rhai: |
    sql::select().from("{name}").field("id").field("name").where(sql::expr("price > 15"))
"#
            );

            let vista = db.vista_factory().from_yaml(&yaml)?;
            assert_eq!(vista.name(), "expensive_products");

            let caps = vista.capabilities();
            assert!(
                !caps.has(Capability::Insert),
                "rhai-sourced vista is read-only"
            );
            assert!(!caps.has(Capability::Update));
            assert!(!caps.has(Capability::Delete));
            assert!(!caps.has(Capability::ConfineWrites));
            assert!(!caps.has(Capability::Import));

            let rows = vista.list_values().await?;
            assert_eq!(rows.len(), 2, "only products with price > 15");
            assert!(rows.contains_key("b"));
            assert!(rows.contains_key("c"));
            assert!(!rows.contains_key("a"));
            Ok(())
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
