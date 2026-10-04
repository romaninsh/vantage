//! Spec `lazy:` columns on SurrealDB: the factory lowers them into Vista
//! computed columns, and a traversal target built through the spec resolver
//! carries the target's own `lazy:` columns and has-many refusal. No server:
//! traversal builds targets offline.
#![cfg(feature = "rhai")]

use std::sync::Arc;

use ciborium::Value as CborValue;
use indexmap::IndexMap;
use surreal_client::{MockSurrealEngine, SurrealClient};
use vantage_surrealdb::surrealdb::SurrealDB;
use vantage_surrealdb::vista::{SurrealSpecResolver, SurrealVistaFactory, SurrealVistaSpec};
use vantage_types::Record;
use vantage_vista::{Vista, VistaFactory};

const TAG: &str = r#"
name: tag
columns:
  id: { type: string, flags: [id] }
  name: { type: string }
  label: { type: string, lazy: 'row.name.to_upper()' }
references:
  events:
    table: event
    kind: has_many
    foreign_key: tag_key
"#;

const EVENT: &str = r#"
name: event
columns:
  id: { type: string, flags: [id] }
  tag: { type: string }
  tag_key: { type: string, lazy: '"tag:" + row.tag' }
references:
  parent:
    table: tag
    kind: has_one
    foreign_key: tag_key
"#;

fn factory() -> SurrealVistaFactory {
    let db = SurrealDB::new(SurrealClient::new(
        Box::new(MockSurrealEngine::new()),
        Some("test".into()),
        Some("test".into()),
    ));
    let specs: IndexMap<String, SurrealVistaSpec> = [("tag", TAG), ("event", EVENT)]
        .into_iter()
        .map(|(name, yaml)| (name.to_string(), serde_yaml_ng::from_str(yaml).unwrap()))
        .collect();
    let resolver: SurrealSpecResolver = Arc::new(move |name: &str| specs.get(name).cloned());
    SurrealVistaFactory::new(db).with_resolver(resolver)
}

fn build(name: &str, yaml: &str) -> Vista {
    factory()
        .build_from_spec(serde_yaml_ng::from_str(yaml).unwrap())
        .unwrap_or_else(|e| panic!("build {name}: {e}"))
}

fn row(pairs: &[(&str, &str)]) -> Record<CborValue> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), CborValue::Text(v.to_string())))
        .collect()
}

#[test]
fn spec_lazy_column_is_computed() {
    let tag = build("tag", TAG);
    assert_eq!(tag.get_column_names(), vec!["id", "name", "label"]);
    assert!(tag.get_column("label").unwrap().is_computed());
}

#[test]
fn has_one_target_carries_its_lazy_columns() {
    let event = build("event", EVENT);
    // The has-one key is computed on the source; the row carries its value.
    let parent = event
        .get_ref(
            "parent",
            &row(&[("id", "event:1"), ("tag", "a"), ("tag_key", "tag:a")]),
        )
        .expect("traverse parent");
    assert!(parent.get_column("label").unwrap().is_computed());
}

#[test]
fn has_many_on_computed_target_key_is_refused() {
    let tag = build("tag", TAG);
    let err = match tag.get_ref("events", &row(&[("id", "tag:a"), ("name", "a")])) {
        Ok(_) => panic!("a has-many joined on a computed column must not traverse"),
        Err(e) => e,
    };
    assert!(err.is_unsupported(), "{err}");
    let column = err.context.get("column").map(|v| v.trim_matches('"'));
    assert_eq!(column, Some("tag_key"), "{err:?}");
}

#[test]
fn bare_ref_target_carries_its_lazy_columns() {
    let tag = build("tag", TAG);
    let events = tag.get_ref_target("events").expect("bare target");
    assert!(events.get_column("tag_key").unwrap().is_computed());
}
