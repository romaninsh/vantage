//! Shell behaviour that needs no server: narrowing, refusals, references and
//! the capability set. The client runs over a mock engine, so these tests
//! inspect the rendered query instead of reading rows.
//!
//! - this file: the fixtures, capabilities, clones and the preview;
//! - `narrow`: conditions, search and order;
//! - `refs`: join and contained traversal;
//! - `scripts`: Rhai on the shell.

mod narrow;
mod refs;
#[cfg(feature = "rhai")]
mod scripts;

use ciborium::Value as CborValue;
use surreal_client::{MockSurrealEngine, SurrealClient};
use vantage_types::Record;
use vantage_vista::{Capability, FilterOp, SortDirection, Vista, VistaFactory};

use crate::surrealdb::SurrealDB;
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

/// A factory whose resolver knows `specs` by name.
fn factory_with(specs: Vec<(&'static str, &'static str)>) -> SurrealVistaFactory {
    let specs: Vec<(&str, SurrealVistaSpec)> =
        specs.into_iter().map(|(n, yaml)| (n, spec(yaml))).collect();
    SurrealVistaFactory::new(test_db()).with_resolver(std::sync::Arc::new(move |name: &str| {
        specs
            .iter()
            .find(|(n, _)| *n == name)
            .map(|(_, s)| s.clone())
    }))
}

fn factory() -> SurrealVistaFactory {
    factory_with(vec![("client", CLIENT), ("bakery", BAKERY)])
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
fn preview_renders_the_select_and_vista_names_the_driver() {
    assert_eq!(clients().driver_name(), "surrealdb");
    assert_eq!(clients().preview().note, None);
    assert!(
        query(&clients()).starts_with("SELECT"),
        "{}",
        query(&clients())
    );
}
