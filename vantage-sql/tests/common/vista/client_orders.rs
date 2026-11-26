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

        /// Narrows `client` to alice and `orders` to hers; other keys pass.
        fn alice_only<T>() -> vantage_table::table::TableInjector<T>
        where
            T: vantage_table::traits::table_source::TableSource,
            T::Value: From<ciborium::Value>,
        {
            std::sync::Arc::new(|cx, table| {
                let column = match cx.key {
                    "client" => "id",
                    "orders" => "client_id",
                    _ => return Ok(()),
                };
                let alice = T::Value::from(ciborium::Value::Text("alice".into()));
                let condition = table.data_source().eq_value_condition(column, alice)?;
                table.add_condition(condition);
                Ok(())
            })
        }

        /// The injector reaches every table a spec builds: the root, a
        /// reference target when traversed, and a `base:` table under a
        /// derived vista.
        #[tokio::test]
        async fn injector_condition_applies_to_root_reference_and_base() -> TestResult {
            let (db, client_t, orders_t) = clients_orders("inject").await;
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
            let derived_spec: $spec = serde_yaml_ng::from_str(&format!(
                r#"
name: order_view
columns: {{}}
{block}:
  base: orders
  inherit:
    columns: [id, client_id, total]
"#
            ))?;

            let factory = db
                .vista_factory()
                .with_resolver(spec_resolver(vec![
                    ("client", client_spec.clone()),
                    ("orders", orders_spec),
                ]))
                .with_injector(alice_only());
            let sorted = |rows: indexmap::IndexMap<String, _>| {
                let mut keys: Vec<String> = rows.into_keys().collect();
                keys.sort();
                keys
            };

            let clients = factory.build_from_spec(client_spec)?;
            assert_eq!(sorted(clients.list_values().await?), ["alice"]);

            let orders = clients.get_ref("orders", None)?;
            assert_eq!(sorted(orders.list_values().await?), ["o1", "o2"]);

            let derived = factory.build_from_spec(derived_spec)?;
            assert_eq!(sorted(derived.list_values().await?), ["o1", "o2"]);
            Ok(())
        }

        type Kinds = std::sync::Arc<std::sync::Mutex<Vec<(String, vantage_table::table::BuildKind)>>>;

        /// Records the key and kind of every build it sees.
        fn recording_kinds<T: vantage_table::traits::table_source::TableSource>(
            kinds: Kinds,
        ) -> vantage_table::table::TableInjector<T> {
            std::sync::Arc::new(move |cx, _table| {
                kinds
                    .lock()
                    .unwrap()
                    .push((cx.key.to_string(), cx.kind.clone()));
                Ok(())
            })
        }

        /// A `base:` table is built as `BuildKind::Base` naming the derived
        /// spec; the derived vista itself is the root build.
        #[tokio::test]
        async fn injector_sees_a_base_build_as_kind_base() -> TestResult {
            let (db, _client_t, orders_t) = clients_orders("base_kind").await;
            let block = $block;
            let orders_spec: $spec = serde_yaml_ng::from_str(&format!(
                r#"
name: orders
columns:
  id: {{ type: string, flags: [id] }}
  client_id: {{ type: string }}
{block}:
  table: {orders_t}
"#
            ))?;
            let derived_spec: $spec = serde_yaml_ng::from_str(&format!(
                r#"
name: order_view
columns: {{}}
{block}:
  base: orders
  inherit:
    columns: [id, client_id]
"#
            ))?;
            let kinds = Kinds::default();
            db.vista_factory()
                .with_resolver(spec_resolver(vec![("orders", orders_spec)]))
                .with_injector(recording_kinds(kinds.clone()))
                .build_from_spec(derived_spec)?;
            assert_eq!(
                *kinds.lock().unwrap(),
                [
                    (
                        "orders".to_string(),
                        vantage_table::table::BuildKind::Base {
                            derived: "order_view".into()
                        }
                    ),
                    ("order_view".to_string(), vantage_table::table::BuildKind::Root),
                ]
            );
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

        /// `client` with `expr:` columns over its orders and its own name, and
        /// a searchable stored `name` next to a searchable `expr:` `label`.
        #[cfg(feature = "rhai")]
        async fn computed_clients(
            suffix: &str,
        ) -> std::result::Result<vantage_vista::Vista, Box<dyn std::error::Error>> {
            let (db, client_t, orders_t) = clients_orders(suffix).await;
            let block = $block;
            let client_spec: $spec = serde_yaml_ng::from_str(&format!(
                r#"
name: client
columns:
  id: {{ type: string, flags: [id] }}
  name: {{ type: string, flags: [title, searchable] }}
  big_order: {{ type: bool, expr: 'table.ref("orders").where("total", 30).exists()' }}
  n_orders: {{ type: int, expr: 'table.ref("orders").count()' }}
  early_name: {{ type: bool, expr: 'sql::expr("{{}} < {{}}", [sql::ident("name"), "B"])' }}
  label: {{ type: string, flags: [searchable], expr: 'sql::concat(["vip-", sql::ident("id")])' }}
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
            Ok(db
                .vista_factory()
                .with_resolver(spec_resolver(vec![("orders", orders_spec)]))
                .build_from_spec(client_spec)?)
        }

        #[cfg(feature = "rhai")]
        async fn ids_of(
            vista: &vantage_vista::Vista,
        ) -> std::result::Result<Vec<String>, Box<dyn std::error::Error>> {
            let mut ids: Vec<String> = vista.list_values().await?.into_keys().collect();
            ids.sort();
            Ok(ids)
        }

        /// A condition on an `expr:` column compares its expression: SQL
        /// has no SELECT alias in WHERE.
        #[cfg(feature = "rhai")]
        #[tokio::test]
        async fn condition_on_expression_columns() -> TestResult {
            use vantage_vista::FilterOp;
            let clients = computed_clients("cond_expr").await?;
            let cases = [
                ("big_order", FilterOp::Eq, ciborium::Value::Bool(true), "EXISTS", "bob"),
                (
                    "n_orders",
                    FilterOp::Gt,
                    ciborium::Value::Integer(1.into()),
                    "COUNT(*)",
                    "alice",
                ),
                ("early_name", FilterOp::Eq, ciborium::Value::Bool(true), "< 'B'", "alice"),
            ];
            for (column, op, value, expression, id) in cases {
                let narrowed = clients.clone().with_condition(column, op, value)?;
                let query = narrowed.preview().query.unwrap_or_default();
                // `label` is the last column the SELECT lists.
                let (_, filter) = query.rsplit_once("label").expect("a projected label");
                assert!(filter.contains(expression), "{column}: {query}");
                assert!(!filter.contains(column), "{column}: {query}");
                assert_eq!(ids_of(&narrowed).await?, [id], "{column}: {query}");
            }
            Ok(())
        }

        /// Quicksearch ORs an `expr:` column's expression with the stored
        /// searchable columns.
        #[cfg(feature = "rhai")]
        #[tokio::test]
        async fn search_matches_expression_and_stored_columns() -> TestResult {
            let clients = computed_clients("search_expr").await?;
            let by_label = clients.clone().with_search("vip-al")?;
            assert_eq!(ids_of(&by_label).await?, ["alice"]);
            let by_name = clients.with_search("Bob")?;
            assert_eq!(ids_of(&by_name).await?, ["bob"]);
            Ok(())
        }

        /// `is` and `is_not` are NULL-safe on a column holding true, false
        /// and NULL; `ne` keeps SQL's `!=`, which a NULL row doesn't match.
        #[tokio::test]
        async fn null_safe_operators() -> TestResult {
            use ciborium::Value;
            use vantage_vista::FilterOp;
            let (db, _, _) = clients_orders("null_safe").await;
            let t = "vista_tri_null_safe";
            for sql in [
                format!("DROP TABLE IF EXISTS {t}"),
                format!("CREATE TABLE {t} (id VARCHAR(20) PRIMARY KEY, t BOOLEAN)"),
                format!("INSERT INTO {t} VALUES ('yes', true), ('no', false), ('none', NULL)"),
            ] {
                sqlx::query(sqlx::AssertSqlSafe(sql))
                    .execute(db.pool())
                    .await?;
            }
            let block = $block;
            let spec: $spec = serde_yaml_ng::from_str(&format!(
                "name: tri\ncolumns:\n  id: {{ type: string, flags: [id] }}\n  \
                 t: {{ type: bool }}\n{block}:\n  table: {t}\n"
            ))?;
            let tri = db.vista_factory().build_from_spec(spec)?;
            let cases = [
                (FilterOp::Is, Value::Null, vec!["none"]),
                (FilterOp::IsNot, Value::Null, vec!["no", "yes"]),
                (FilterOp::IsNot, Value::Bool(true), vec!["no", "none"]),
                (FilterOp::Is, Value::Bool(true), vec!["yes"]),
                (FilterOp::Ne, Value::Bool(true), vec!["no"]),
            ];
            for (op, value, expected) in cases {
                let narrowed = tri.clone().with_condition("t", op, value.clone())?;
                let mut ids: Vec<String> = narrowed.list_values().await?.into_keys().collect();
                ids.sort();
                assert_eq!(ids, expected, "{op:?} {value:?}: {:?}", narrowed.preview().query);
            }
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
