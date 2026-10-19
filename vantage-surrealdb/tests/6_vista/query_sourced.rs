//! Query-sourced vistas: a `rhai:` source, a `modify:` script, and
//! `base:`/`inherit:` derivation. A query source makes a vista read-only.

use std::sync::Arc;

use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_dataset::prelude::*;
use vantage_expressions::ExprDataSource;
use vantage_surrealdb::surreal_expr;
use vantage_surrealdb::surrealdb::SurrealDB;
use vantage_surrealdb::vista::SurrealVistaSpec;
use vantage_vista::{Capability, SpecResolver, Vista, VistaFactory};

use super::{TestResult, seed_row, setup};

/// The `args` example from the book's "Tables Defined by a Query" page.
const ORDERS_BY_BAKERY: &str = r#"
name: bakery_orders
columns:
  id: { type: thing, flags: [id] }
surreal:
  rhai: |
    let q = surreal::select().from("order");
    if "bakery" in args && args.bakery != "" {
        q = q.where(surreal::expr("bakery.name = {}", [args.bakery]));
    }
    q
"#;

/// `yaml` parsed, with `args` set on its driver block the way a host sets them.
fn spec_with_args(yaml: &str, args: &[(&str, &str)]) -> SurrealVistaSpec {
    let mut spec: SurrealVistaSpec = serde_yaml_ng::from_str(yaml).expect("spec yaml");
    let block = spec.driver.surreal.as_mut().expect("surreal block");
    block.args = args
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    spec
}

async fn sorted_ids(vista: &Vista) -> vantage_core::Result<Vec<String>> {
    let mut ids: Vec<String> = vista.list_values().await?.into_keys().collect();
    ids.sort();
    Ok(ids)
}

async fn seed_client(db: &SurrealDB, id: &str, name: &str) -> TestResult {
    let name_owned = name.to_string();
    db.execute(&surreal_expr!(
        &format!("CREATE client:{} SET name = {{}}", id),
        name_owned
    ))
    .await?;
    Ok(())
}

/// `client_id` is a record link (`Thing`) so a `GROUP BY client_id` aggregate
/// re-keys cleanly onto `client:<id>` — SurrealDB's native identity model.
async fn seed_order(db: &SurrealDB, id: &str, client: &str, total: i64) -> TestResult {
    db.execute(&surreal_expr!(
        &format!(
            "CREATE orders:{} SET client_id = client:{}, total = {{}}",
            id, client
        ),
        total
    ))
    .await?;
    Ok(())
}

/// A resolver that knows one spec, under `name`.
fn resolver(name: &str, spec: SurrealVistaSpec) -> Arc<dyn SpecResolver<SurrealVistaSpec>> {
    let map: IndexMap<String, SurrealVistaSpec> = [(name.to_string(), spec)].into_iter().collect();
    let map = Arc::new(map);
    Arc::new(move |name: &str| map.get(name).cloned())
}

/// A vista whose source is a Rhai-built SELECT (not a physical table). The
/// script filters `price > 15`, so only two products surface, and the vista is
/// read-only.
#[tokio::test]
async fn vista_yaml_rhai_source_is_read_only_and_filters() -> TestResult {
    let (db, _) = setup().await;
    seed_row(&db, "product", "a", "A", 10, false).await?;
    seed_row(&db, "product", "b", "B", 20, false).await?;
    seed_row(&db, "product", "c", "C", 30, false).await?;

    let yaml = r#"
name: expensive_products
columns:
  id:
    type: thing
    flags: [id]
  name:
    type: string
surreal:
  rhai: |
    surreal::select().from("product").field("id").field("name").where(surreal::expr("price > 15"))
"#;

    let vista = db.vista_factory().from_yaml(yaml)?;
    assert_eq!(vista.name(), "expensive_products");

    let caps = vista.capabilities();
    assert!(
        !caps.has(Capability::Insert),
        "rhai-sourced vista is read-only"
    );
    assert!(!caps.has(Capability::Update));
    assert!(!caps.has(Capability::Delete));
    assert!(!caps.has(Capability::Subscribe));

    let rows = vista.list_values().await?;
    assert_eq!(rows.len(), 2, "only products with price > 15");
    assert!(rows.contains_key("product:b"));
    assert!(rows.contains_key("product:c"));
    assert!(!rows.contains_key("product:a"));
    Ok(())
}

/// Each verb on `self` starts from the built vista, and the script's value is
/// the result: the discarded `cheap` handle (which would leave no row) must
/// not narrow the `live` one built from `self` after it.
#[tokio::test]
async fn modify_script_resolves_self_twice() -> TestResult {
    let (db, _) = setup().await;
    seed_row(&db, "product", "a", "A", 10, false).await?;
    seed_row(&db, "product", "b", "B", 20, false).await?;
    seed_row(&db, "product", "c", "C", 30, true).await?;

    let yaml = r#"
name: product
columns:
  id: { type: thing, flags: [id] }
  name: { type: string }
  price: { type: int }
  is_deleted: { type: bool }
surreal:
  modify: |
    let cheap = self.where("price", "<", 15);
    let live = self.with_condition(surreal::ident("is_deleted") == false);
    live.where("price", ">", 15)
"#;
    let vista = db.vista_factory().from_yaml(yaml)?;
    let ids: Vec<String> = vista.list_values().await?.into_keys().collect();
    assert_eq!(ids, vec!["product:b"]);
    assert_eq!(vista.query().conditions.len(), 1);
    assert_eq!(vista.query().native.len(), 1);
    Ok(())
}

/// A derived vista: `base: client` resolves eagerly through the factory's
/// resolver, the base table's `select()` is seeded into the Rhai engine as
/// `base` (transform mode), and the listed columns are inherited from the base.
/// The query source makes it read-only.
#[tokio::test]
async fn vista_yaml_base_inherits_columns_and_transforms() -> TestResult {
    let (db, _) = setup().await;
    seed_client(&db, "alice", "Alice").await?;
    seed_client(&db, "bob", "Bob").await?;

    let client_yaml = r#"
name: client
columns:
  id: { type: thing, flags: [id] }
  name: { type: string, flags: [title] }
"#;
    let vip_yaml = r#"
name: vip_clients
columns: {}
surreal:
  base: client
  inherit:
    columns: [id, name]
  rhai: |
    base.where(surreal::expr("name = 'Alice'"))
"#;

    let client_spec: SurrealVistaSpec = serde_yaml_ng::from_str(client_yaml)?;
    let vip_spec: SurrealVistaSpec = serde_yaml_ng::from_str(vip_yaml)?;

    let vista = db
        .vista_factory()
        .with_resolver(resolver("client", client_spec))
        .build_from_spec(vip_spec)?;

    // Query-sourced → read-only.
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
    let alice = rows.get("client:alice").expect("alice present");
    assert_eq!(alice.get("name"), Some(&CborValue::Text("Alice".into())));
    Ok(())
}

/// The headline "derived aggregate" case: a `debtors`-style vista derived from
/// `orders`, grouping by client and summing totals into a declared `total_due`
/// column, re-keyed onto the `client_id` record link.
#[tokio::test]
async fn vista_yaml_base_aggregates_into_declared_column() -> TestResult {
    let (db, _) = setup().await;
    seed_order(&db, "o1", "alice", 10).await?;
    seed_order(&db, "o2", "alice", 20).await?;
    seed_order(&db, "o3", "bob", 30).await?;

    let orders_yaml = r#"
name: orders
columns:
  id: { type: thing, flags: [id] }
  client_id: { type: thing }
  total: { type: int }
"#;
    let totals_yaml = r#"
name: client_totals
id_column: client_id
columns:
  total_due: { type: int }
surreal:
  base: orders
  inherit:
    columns: [client_id]
  rhai: |
    base.clear_fields().field("client_id").expression(surreal::expr("math::sum(total) AS total_due")).group_by(surreal::expr("client_id"))
"#;

    let orders_spec: SurrealVistaSpec = serde_yaml_ng::from_str(orders_yaml)?;
    let totals_spec: SurrealVistaSpec = serde_yaml_ng::from_str(totals_yaml)?;

    let vista = db
        .vista_factory()
        .with_resolver(resolver("orders", orders_spec))
        .build_from_spec(totals_spec)?;

    // Re-keyed by client_id; one row per client with the summed total.
    let rows = vista.list_values().await?;
    assert_eq!(rows.len(), 2, "one row per client");
    let alice = rows.get("client:alice").expect("alice total");
    assert_eq!(
        alice.get("total_due"),
        Some(&CborValue::Integer(30i64.into()))
    );
    let bob = rows.get("client:bob").expect("bob total");
    assert_eq!(
        bob.get("total_due"),
        Some(&CborValue::Integer(30i64.into()))
    );
    Ok(())
}

/// The book's `args` example: with `bakery` set the query gains the
/// condition, with the name bound as a parameter, and without it the script
/// skips the condition instead of failing.
#[tokio::test]
async fn args_reach_a_rhai_source_and_bind_as_parameters() -> TestResult {
    let (db, _) = setup().await;
    for sql in [
        "CREATE bakery:breg SET name = 'Breg'",
        "CREATE bakery:other SET name = 'Other'",
        "CREATE ⟨order⟩:o1 SET bakery = bakery:breg",
        "CREATE ⟨order⟩:o2 SET bakery = bakery:other",
    ] {
        db.execute(&surreal_expr!(sql)).await?;
    }
    let open = |args: &[(&str, &str)]| {
        db.vista_factory()
            .build_from_spec(spec_with_args(ORDERS_BY_BAKERY, args))
    };

    let breg = open(&[("bakery", "Breg")])?;
    assert_eq!(sorted_ids(&breg).await?, ["order:o1"]);

    for unset in [open(&[("bakery", "")])?, open(&[])?] {
        assert_eq!(sorted_ids(&unset).await?, ["order:o1", "order:o2"]);
    }

    // A bound value is compared whole, so quotes in it can't widen the query.
    let crafted = open(&[("bakery", "Breg\" OR true OR \"")])?;
    assert!(sorted_ids(&crafted).await?.is_empty());
    Ok(())
}

/// Arguments reach a `base:` transform as they reach a `rhai:` source.
#[tokio::test]
async fn args_reach_a_base_transform() -> TestResult {
    let (db, _) = setup().await;
    seed_client(&db, "alice", "Alice").await?;
    seed_client(&db, "bob", "Bob").await?;

    let client_spec: SurrealVistaSpec = serde_yaml_ng::from_str(
        r#"
name: client
columns:
  id: { type: thing, flags: [id] }
  name: { type: string }
"#,
    )?;
    let picked_yaml = r#"
name: picked_clients
columns: {}
surreal:
  base: client
  inherit:
    columns: [id, name]
  rhai: |
    base.where(surreal::expr("name = {}", [args.who]))
"#;
    let vista = db
        .vista_factory()
        .with_resolver(resolver("client", client_spec))
        .build_from_spec(spec_with_args(picked_yaml, &[("who", "Bob")]))?;
    assert_eq!(sorted_ids(&vista).await?, ["client:bob"]);
    Ok(())
}

/// A `modify:` script runs without `args`, so one that reads them fails the
/// build even when the host set some.
#[tokio::test]
async fn args_do_not_reach_a_modify_script() -> TestResult {
    let (db, _) = setup().await;
    let yaml = r#"
name: client
columns:
  id: { type: thing, flags: [id] }
  name: { type: string }
surreal:
  table: client
  modify: |
    self.where("name", args.who)
"#;
    let built = db
        .vista_factory()
        .build_from_spec(spec_with_args(yaml, &[("who", "Bob")]));
    assert!(built.is_err(), "a modify script has no `args` in scope");
    Ok(())
}

/// Without `rhai:`, `base:` uses the base query as it is; the vista is still
/// read-only.
#[tokio::test]
async fn base_without_rhai_reads_the_base_query_as_it_is() -> TestResult {
    let (db, _) = setup().await;
    seed_client(&db, "alice", "Alice").await?;
    seed_client(&db, "bob", "Bob").await?;

    let client_spec: SurrealVistaSpec = serde_yaml_ng::from_str(
        r#"
name: client
columns:
  id: { type: thing, flags: [id] }
  name: { type: string }
"#,
    )?;
    let all_yaml = r#"
name: all_clients
columns: {}
surreal:
  base: client
  inherit:
    columns: [id, name]
"#;
    let vista = db
        .vista_factory()
        .with_resolver(resolver("client", client_spec))
        .build_from_spec(serde_yaml_ng::from_str(all_yaml)?)?;
    assert!(!vista.capabilities().has(Capability::Insert));
    assert_eq!(sorted_ids(&vista).await?, ["client:alice", "client:bob"]);
    Ok(())
}

/// A query-sourced vista has no table to write to or subscribe to, so it
/// advertises the reads only, and `watch` is refused.
#[tokio::test]
async fn query_sourced_vista_advertises_the_reads_only() -> TestResult {
    let (db, _) = setup().await;
    seed_row(&db, "product", "a", "A", 10, false).await?;
    let yaml = r#"
name: all_products
columns:
  id: { type: thing, flags: [id] }
  name: { type: string }
surreal:
  rhai: |
    surreal::select().from("product").field("id").field("name")
"#;
    let vista = db.vista_factory().from_yaml(yaml)?;
    let caps = vista.capabilities();
    for read in [
        Capability::Order,
        Capability::Search,
        Capability::FilterOps,
        Capability::NativeConditions,
        Capability::Window,
        Capability::Traverse,
    ] {
        assert!(caps.has(read), "{read:?}");
    }
    for table_only in [
        Capability::Insert,
        Capability::Update,
        Capability::Delete,
        Capability::ConfineWrites,
        Capability::Subscribe,
    ] {
        assert!(!caps.has(table_only), "{table_only:?}");
    }
    let err = vista.watch().await.map(drop).expect_err("no LIVE source");
    assert!(err.is_unsupported(), "{err:?}");
    Ok(())
}

/// A script that fails, or that doesn't end on a `select()`, fails the build.
#[test]
fn rhai_source_that_fails_or_ends_on_no_select_fails_the_build() {
    let db = SurrealDB::new(surreal_client::SurrealMockBuilder::new().build());
    for script in [r#"surreal::select().from("product").nope()"#, "42"] {
        let yaml = format!(
            "name: broken\ncolumns:\n  id: {{ type: thing, flags: [id] }}\nsurreal:\n  rhai: '{script}'\n"
        );
        assert!(
            db.vista_factory().from_yaml(&yaml).is_err(),
            "{script} must fail the build"
        );
    }
}

/// An `expr:` script is evaluated once, at build time, like a `rhai:` source:
/// one that fails, or that yields no expression, fails the build.
#[test]
fn expr_column_that_fails_or_yields_no_expression_fails_the_build() {
    let db = SurrealDB::new(surreal_client::SurrealMockBuilder::new().build());
    for script in ["surreal::nope()", "42"] {
        let yaml = format!(
            "name: line\ncolumns:\n  id: {{ type: thing, flags: [id] }}\n  total: {{ type: int, expr: '{script}' }}\n"
        );
        assert!(
            db.vista_factory().from_yaml(&yaml).is_err(),
            "{script} must fail the build"
        );
    }
}

/// A derived vista builds only the relations it inherits, so a reference it
/// declares itself would have no target: the build refuses it.
#[test]
fn derived_vista_declaring_its_own_reference_fails_the_build() {
    let db = SurrealDB::new(surreal_client::SurrealMockBuilder::new().build());
    let client_spec: SurrealVistaSpec = serde_yaml_ng::from_str(
        r#"
name: client
columns:
  id: { type: thing, flags: [id] }
  name: { type: string }
"#,
    )
    .expect("client yaml");
    let derived: SurrealVistaSpec = serde_yaml_ng::from_str(
        r#"
name: vip_clients
columns: {}
surreal:
  base: client
  inherit:
    columns: [id, name]
references:
  orders:
    table: order
    kind: has_many
    foreign_key: client
"#,
    )
    .expect("derived yaml");
    let built = db
        .vista_factory()
        .with_resolver(resolver("client", client_spec))
        .build_from_spec(derived);
    assert!(built.is_err());
}

/// The book's `with_condition` example: a `modify:` script narrows a real
/// table by a native SurrealQL expression and sorts it, and the vista stays
/// writable.
#[tokio::test]
async fn modify_with_condition_narrows_a_writable_table() -> TestResult {
    let (db, _) = setup().await;
    for (id, name, paying) in [
        ("marty", "Marty", true),
        ("biff", "Biff", false),
        ("doc", "Doc", true),
    ] {
        let name = name.to_string();
        db.execute(&surreal_expr!(
            &format!("CREATE client:{id} SET name = {{}}, is_paying_client = {{}}"),
            name,
            paying
        ))
        .await?;
    }
    let yaml = r#"
name: client
columns:
  id: { type: thing, flags: [id] }
  name: { type: string, flags: [title] }
  is_paying_client: { type: bool }
surreal:
  table: client
  modify: |
    self.with_condition(surreal::ident("is_paying_client") == true)
       .sort("name", "asc")
"#;
    let vista = db.vista_factory().from_yaml(yaml)?;
    assert!(vista.capabilities().has(Capability::Insert));

    let names: Vec<CborValue> = vista
        .list_values()
        .await?
        .into_values()
        .filter_map(|row| row.get("name").cloned())
        .collect();
    assert_eq!(
        names,
        [
            CborValue::Text("Doc".into()),
            CborValue::Text("Marty".into())
        ]
    );
    Ok(())
}

/// A rule script narrows the table itself, before any vista exists, with
/// the same `with_condition` a `modify:` script has.
#[test]
fn rule_with_condition_narrows_the_table() {
    use vantage_rhai::{Env, Host, Limits};
    use vantage_surrealdb::thing::Thing;
    use vantage_surrealdb::types::{AnySurrealType, SurrealType};
    use vantage_surrealdb::vista::register_rule_conditions;
    use vantage_table::rule::{RuleCodec, TableRuleTarget};
    use vantage_table::table::Table;
    use vantage_types::EmptyEntity;
    use vantage_vista::rhai::{RuleHandle, RuleVerb, eval_rule_script, register_rule_vocab};

    let db = SurrealDB::new(surreal_client::SurrealMockBuilder::new().build());
    let table = Table::<SurrealDB, EmptyEntity>::new("client", db)
        .with_id_column("id")
        .with_column_of::<i64>("bakery_id");
    let target = TableRuleTarget::new(
        table,
        RuleCodec {
            to_cbor: AnySurrealType::into_value,
            from_cbor: AnySurrealType::from,
            id_cbor: Thing::to_cbor,
        },
    );
    let handle = RuleHandle::new(Box::new(target.clone()));
    let host = Host::builder(Limits::background())
        .vocab_fn(register_rule_vocab)
        .vocab_fn(register_rule_conditions)
        .build();
    eval_rule_script(
        &host,
        r#"table.with_condition(surreal::expr("bakery_id = {}", [3]))"#,
        Env::new(),
        &handle,
    )
    .unwrap();

    let table = target.into_table();
    assert_eq!(
        table.select().preview(),
        "SELECT id, bakery_id FROM client WHERE bakery_id = 3"
    );
    assert_eq!(
        handle.verbs(),
        [RuleVerb::Condition {
            described: "bakery_id = 3".into()
        }]
    );
}
