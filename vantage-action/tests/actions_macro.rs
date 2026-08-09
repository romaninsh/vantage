//! Compile-and-run coverage for `#[actions]`.
//!
//! The macro is only exercised when something expands it, so without
//! this file CI type-checks the proc macro's own source and nothing it
//! produces. Every test here exists to keep a generated-code shape
//! compiling; the assertions on descriptors are secondary.

use std::sync::Arc;
use std::sync::Mutex;

use async_trait::async_trait;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use vantage_action::{ActionTarget, Target, actions};
use vantage_core::Result;

#[derive(Deserialize, JsonSchema)]
struct Greeting {
    who: String,
}

#[derive(Serialize, JsonSchema)]
struct Greeted {
    said: String,
}

/// A dependency passed by reference — the constructor takes it as `Arc<T>`.
#[derive(Default)]
struct Recorder {
    seen: Mutex<Vec<String>>,
}

#[actions]
#[async_trait]
trait Greeter {
    /// Greet one record.
    #[action(name = "greet_one", table = "person")]
    async fn greet_one(&self, #[id] id: &str, req: Greeting, rec: &Recorder) -> Result<Greeted>;

    /// Greet the whole table.
    #[action(name = "greet_all", table = "person")]
    async fn greet_all(&self, req: Greeting) -> Result<Greeted>;
}

struct Store;

#[async_trait]
impl Greeter for Store {
    async fn greet_one(&self, id: &str, req: Greeting, rec: &Recorder) -> Result<Greeted> {
        rec.seen.lock().unwrap().push(id.to_string());
        Ok(Greeted {
            said: format!("hello {} at {id}", req.who),
        })
    }

    async fn greet_all(&self, req: Greeting) -> Result<Greeted> {
        Ok(Greeted {
            said: format!("hello all from {}", req.who),
        })
    }
}

#[tokio::test]
async fn record_action_passes_the_id_and_the_dependency() {
    let recorder = Arc::new(Recorder::default());
    let action = greet_one_action(Store, Arc::clone(&recorder));

    assert_eq!(action.descriptor().name, "greet_one");
    assert_eq!(
        action.descriptor().target,
        ActionTarget::Record {
            table: "person".to_string()
        }
    );
    assert_eq!(action.descriptor().description, "Greet one record.");

    let out = action
        .invoke(
            Target::Id("person:ab".to_string()),
            serde_json::json!({ "who": "sam" }),
        )
        .await
        .expect("invoke");

    // The `<table>:` prefix is stripped before the body sees the key.
    assert_eq!(recorder.seen.lock().unwrap().as_slice(), ["ab"]);
    match out {
        vantage_action::ActionOutput::Value(v) => {
            assert_eq!(v["said"], "hello sam at ab");
        }
        other => panic!("expected a value output, got {other:?}"),
    }
}

#[tokio::test]
async fn table_action_takes_no_id() {
    let action = greet_all_action(Store);

    assert_eq!(
        action.descriptor().target,
        ActionTarget::Table {
            table: "person".to_string()
        }
    );

    let out = action
        .invoke(Target::None, serde_json::json!({ "who": "sam" }))
        .await
        .expect("invoke");
    match out {
        vantage_action::ActionOutput::Value(v) => {
            assert_eq!(v["said"], "hello all from sam");
        }
        other => panic!("expected a value output, got {other:?}"),
    }
}
