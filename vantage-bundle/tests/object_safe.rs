//! The application holds bundles as trait objects with the connection
//! pinned. If that stops compiling, every consumer's registry breaks —
//! and the associated type exists precisely to make it work, so it is
//! worth a test rather than an assumption.

use std::collections::HashMap;
use std::sync::Arc;

use vantage_bundle::{BuiltinAction, BundleTable, ModelBundle};

/// Stands in for a real database handle.
struct FakeDb;

struct Demo;

impl ModelBundle for Demo {
    type Connection = FakeDb;

    fn name(&self) -> &str {
        "demo"
    }

    fn tables(&self) -> Vec<BundleTable> {
        vec![BundleTable {
            key: "widget".into(),
            datasource: "shop".into(),
            title: Some("Widget".into()),
        }]
    }

    fn build_vista(
        &self,
        table_key: &str,
        _db: &FakeDb,
    ) -> vantage_core::Result<vantage_vista::Vista> {
        Err(vantage_core::error!(
            "no vista in a test",
            table = table_key
        ))
    }

    fn actions(&self) -> Vec<Arc<dyn BuiltinAction<Connection = FakeDb>>> {
        vec![Arc::new(Noop)]
    }
}

struct Noop;

#[async_trait::async_trait]
impl BuiltinAction for Noop {
    type Connection = FakeDb;

    fn name(&self) -> &str {
        "noop"
    }

    fn datasource(&self) -> &str {
        "shop"
    }

    async fn run(
        &self,
        _db: &FakeDb,
        args: serde_json::Value,
    ) -> vantage_core::Result<serde_json::Value> {
        Ok(args)
    }
}

/// The shape an application's registry uses.
type Registry = Vec<Arc<dyn ModelBundle<Connection = FakeDb>>>;

#[test]
fn a_bundle_is_usable_as_a_trait_object_with_its_connection_pinned() {
    let registry: Registry = vec![Arc::new(Demo)];
    let bundle = &registry[0];

    assert_eq!(bundle.name(), "demo");
    assert_eq!(bundle.tables()[0].key, "widget");
    // The default `model_actions` body has to stay callable through the
    // object too — it mentions `Self::Connection` in its signature.
    assert!(bundle.model_actions(&HashMap::new()).is_empty());

    let action = &bundle.actions()[0];
    assert_eq!(action.name(), "noop");
    assert_eq!(action.datasource(), "shop");
}

#[tokio::test]
async fn an_action_runs_through_the_object() {
    let action: Arc<dyn BuiltinAction<Connection = FakeDb>> = Arc::new(Noop);
    let out = action
        .run(&FakeDb, serde_json::json!({ "n": 1 }))
        .await
        .expect("noop");
    assert_eq!(out, serde_json::json!({ "n": 1 }));
}
