//! Shell behaviour that needs no server: narrowing, refusals, references and
//! the capability set. The client runs over a mock engine, so these tests
//! inspect the rendered query instead of reading rows.

use ciborium::Value as CborValue;
use surreal_client::{MockSurrealEngine, SurrealClient};
use vantage_table::table::Table;
use vantage_types::{EmptyEntity, Record};
use vantage_vista::{Capability, FilterOp, NativeCondition, SortDirection, Vista, VistaFactory};

use crate::surrealdb::SurrealDB;
use crate::types::AnySurrealType;
use crate::vista::{SurrealVistaFactory, SurrealVistaSpec};

fn test_db() -> SurrealDB {
    let client = SurrealClient::new(
        Box::new(MockSurrealEngine::new()),
        Some("test".into()),
        Some("test".into()),
    );
    SurrealDB::new(client)
}

const CLIENT: &str = r#"
name: client
columns:
  id: { type: thing, flags: [id, orderable] }
  name: { type: string, flags: [title, searchable] }
  city: { type: string }
  bakery: { type: thing }
references:
  bakery:
    table: bakery
    kind: has_one
    foreign_key: bakery
"#;

const BAKERY: &str = r#"
name: bakery
columns:
  id: { type: thing, flags: [id] }
  name: { type: string, flags: [title] }
references:
  clients:
    table: client
    kind: has_many
    foreign_key: bakery
"#;

fn spec(yaml: &str) -> SurrealVistaSpec {
    serde_yaml_ng::from_str(yaml).expect("yaml parse")
}

fn factory() -> SurrealVistaFactory {
    let specs = [("client", spec(CLIENT)), ("bakery", spec(BAKERY))];
    SurrealVistaFactory::new(test_db()).with_resolver(std::sync::Arc::new(move |name: &str| {
        specs
            .iter()
            .find(|(n, _)| *n == name)
            .map(|(_, s)| s.clone())
    }))
}

fn clients() -> Vista {
    factory()
        .build_from_spec(spec(CLIENT))
        .expect("build client")
}

fn query(vista: &Vista) -> String {
    vista.preview().query.expect("preview renders the select")
}

fn row(pairs: &[(&str, CborValue)]) -> Record<CborValue> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.clone()))
        .collect()
}

#[test]
fn table_vista_advertises_reads_writes_and_subscribe() {
    let caps = *clients().capabilities();
    let advertised: Vec<Capability> = caps.iter().collect();
    for capability in [
        Capability::Insert,
        Capability::Update,
        Capability::Delete,
        Capability::ConfineWrites,
        Capability::Subscribe,
        Capability::Order,
        Capability::Search,
        Capability::FilterOps,
        Capability::NativeConditions,
        Capability::Window,
        Capability::Traverse,
    ] {
        assert!(caps.has(capability), "{capability} in {advertised:?}");
    }
    assert!(!caps.has(Capability::Cursor), "{advertised:?}");
    assert!(!caps.has(Capability::Import), "{advertised:?}");
}

#[cfg(feature = "rhai")]
#[tokio::test]
async fn query_sourced_vista_is_read_only_and_unwatchable() {
    let yaml = r#"
name: hv_clients
columns:
  id: { type: thing, flags: [id] }
  name: { type: string }
surreal:
  rhai: |
    select().from("client")
"#;
    let vista = factory().build_from_spec(spec(yaml)).expect("build");
    let caps = *vista.capabilities();
    for capability in [
        Capability::Insert,
        Capability::Update,
        Capability::Delete,
        Capability::ConfineWrites,
        Capability::Subscribe,
    ] {
        assert!(!caps.has(capability), "{capability}");
    }
    assert!(caps.has(Capability::NativeConditions));
    let err = vista.watch().await.err().expect("watch is refused");
    assert!(err.is_unsupported(), "{err:?}");
}

#[test]
fn clone_owns_its_narrowing() {
    let base = clients();
    let narrowed = base
        .clone()
        .with_condition("city", FilterOp::Eq, "hv")
        .unwrap()
        .with_order("name", SortDirection::Descending)
        .unwrap();
    assert!(
        query(&narrowed).contains("city = \"hv\""),
        "{}",
        query(&narrowed)
    );
    assert!(!query(&base).contains("WHERE"), "{}", query(&base));
    assert!(!base.is_narrowed());

    let other = narrowed.clone().with_search("marty").unwrap();
    assert!(query(&other).contains("marty"), "{}", query(&other));
    assert!(!query(&narrowed).contains("marty"), "{}", query(&narrowed));
}

#[test]
fn search_and_order_replace_and_clear() {
    let mut vista = clients();
    vista.set_search(Some("doc")).unwrap();
    vista.set_search(Some("biff")).unwrap();
    let q = query(&vista);
    assert!(q.contains("biff") && !q.contains("doc"), "{q}");
    vista.set_search(None).unwrap();
    assert!(!query(&vista).contains("biff"), "{}", query(&vista));

    vista
        .set_order(Some(("name", SortDirection::Ascending)))
        .unwrap();
    assert!(query(&vista).contains("ORDER BY"), "{}", query(&vista));
    vista.set_order(None).unwrap();
    assert!(!query(&vista).contains("ORDER BY"), "{}", query(&vista));
}

#[test]
fn id_condition_coerces_a_string_into_a_record_id() {
    let vista = clients().with_id("client:marty").unwrap();
    assert!(
        query(&vista).contains("id = client:marty"),
        "{}",
        query(&vista)
    );
    let bare = clients().with_id("marty").unwrap();
    assert!(
        query(&bare).contains("id = client:marty"),
        "{}",
        query(&bare)
    );
}

#[test]
fn unknown_column_is_a_normal_error_and_records_nothing() {
    let mut vista = clients();
    let err = vista.add_condition("nope", FilterOp::Eq, "x").unwrap_err();
    assert!(!err.is_unsupported(), "{err:?}");
    assert!(!vista.is_narrowed());
    let err = clients().with_order("nope", SortDirection::Ascending).err();
    assert!(err.is_some_and(|e| !e.is_unsupported()));
}

#[test]
fn not_in_set_is_refused_for_local_evaluation() {
    let mut vista = clients();
    let value = CborValue::Array(vec!["hv".into()]);
    let err = vista
        .add_condition("city", FilterOp::NotInSet, value)
        .unwrap_err();
    assert!(err.is_unsupported(), "{err:?}");
    assert!(vista.query().conditions.is_empty());
}

#[test]
fn native_condition_takes_an_expression_and_rejects_other_types() {
    let expr = crate::surreal_expr!("city != {}", (AnySurrealType::from(CborValue::from("tp"))));
    let key = expr.preview();
    let vista = clients()
        .with_native_condition(NativeCondition::new(expr, key.clone()))
        .unwrap();
    assert_eq!(vista.query().native, vec![key]);
    assert!(
        query(&vista).contains("city != \"tp\""),
        "{}",
        query(&vista)
    );

    let err = clients()
        .with_native_condition(NativeCondition::new(42_u8, "not-surreal"))
        .map(drop)
        .expect_err("foreign type");
    assert!(err.is_incorrect_usage(), "{err:?}");
}

#[test]
fn has_many_narrows_by_a_coerced_parent_id() {
    let bakeries = factory().build_from_spec(spec(BAKERY)).unwrap();
    let parent = row(&[("id", "bakery:hill_valley".into())]);
    let clients = bakeries.get_ref("clients", Some(&parent)).unwrap();
    assert!(clients.is_narrowed());
    assert_eq!(clients.query().conditions[0].column, "bakery");
    assert!(
        query(&clients).contains("bakery = bakery:hill_valley"),
        "the string id is a record literal: {}",
        query(&clients)
    );

    let bare = bakeries.get_ref("clients", None).unwrap();
    assert!(!bare.is_narrowed());
}

#[test]
fn has_one_narrows_by_the_target_id() {
    let parent = row(&[
        ("id", "client:marty".into()),
        ("bakery", "bakery:hv".into()),
    ]);
    let bakery = clients().get_ref("bakery", Some(&parent)).unwrap();
    assert_eq!(bakery.query().conditions[0].column, "id");
    assert!(
        query(&bakery).contains("id = bakery:hv"),
        "{}",
        query(&bakery)
    );

    let err = clients()
        .get_ref("nope", None)
        .map(drop)
        .expect_err("unknown relation");
    assert!(!err.is_unsupported(), "{err:?}");
}

#[test]
fn contained_relation_needs_a_parent_row() {
    let table = Table::<SurrealDB, EmptyEntity>::new("contained_test", test_db())
        .with_id_column("id")
        .with_column_of::<AnySurrealType>("lines")
        .with_contained_many(
            "lines",
            "lines",
            |db| Table::new("lines", db).with_column_of::<i64>("a"),
            None,
        );
    let vista = SurrealVistaFactory::new(test_db())
        .from_table(table)
        .unwrap();
    assert!(
        vista
            .reference("lines")
            .is_some_and(|r| r.kind.is_contained())
    );

    let err = vista
        .get_ref("lines", None)
        .map(drop)
        .expect_err("needs a row");
    assert!(err.is_incorrect_usage(), "{err:?}");

    let lines = CborValue::Array(vec![CborValue::Map(vec![("a".into(), 1.into())])]);
    let parent = row(&[("id", "contained_test:t1".into()), ("lines", lines)]);
    assert!(vista.get_ref("lines", Some(&parent)).is_ok());
}

#[test]
fn preview_names_the_driver_and_renders_the_select() {
    let preview = clients().preview();
    assert_eq!(preview.driver, "surrealdb");
    assert!(
        query(&clients()).starts_with("SELECT"),
        "{}",
        query(&clients())
    );
}

/// `me` is an engine constant from `register_rhai_extensions`: a script
/// reads it with an empty `Env`, inside its own functions too.
#[cfg(feature = "rhai")]
#[test]
fn me_is_an_engine_constant_of_the_script_host() {
    use vantage_rhai::{Block, Env};

    let host = factory().script_host();
    for code in ["me[\"name\"]", "fn anchor() { me[\"name\"] } anchor()"] {
        let value = host
            .compile(&Block::from(code))
            .and_then(|script| script.eval(&Env::new()))
            .unwrap_or_else(|e| panic!("{code}: {e}"));
        assert!(
            value.is::<crate::rhai_engine::RhaiExpr>(),
            "{code}: {}",
            value.type_name()
        );
    }
}

#[cfg(feature = "rhai")]
#[test]
fn me_reads_inside_a_modify_script() {
    let yaml = r#"
name: client
columns:
  id: { type: thing, flags: [id] }
  name: { type: string }
surreal:
  table: client
  modify: |
    self.with_condition(me["name"] == "Marty")
"#;
    let vista = factory()
        .build_from_spec(spec(yaml))
        .expect("modify with me");
    let native = &vista.query().native;
    assert_eq!(native.len(), 1);
    assert!(native[0].contains("name"), "{native:?}");
}
