//! The rule handle: a script narrows a table and registers write hooks, and
//! has no verb that reads, writes or reaches another table.

use std::any::Any;
use std::sync::{Arc, Mutex};

use ciborium::Value as CborValue;
use vantage_core::Result;
use vantage_rhai::rhai::Map as RhaiMap;
use vantage_rhai::{Env, Host, Limits};
use vantage_vista::FilterOp;
use vantage_vista::rhai::{
    HookOn, RuleHandle, RuleHookFn, RuleHookInput, RuleTarget, RuleVerb, eval_rule_script,
    register_rule_vocab,
};

/// Records what the script asked for and keeps the hooks so a test can fire them.
#[derive(Clone, Default)]
struct Recorder {
    calls: Arc<Mutex<Vec<String>>>,
    hooks: Arc<Mutex<Vec<(HookOn, RuleHookFn)>>>,
}

impl Recorder {
    fn calls(&self) -> Vec<String> {
        self.calls.lock().unwrap().clone()
    }

    fn fire(&self, on: HookOn, input: RuleHookInput) -> std::result::Result<(), String> {
        let hooks = self.hooks.lock().unwrap();
        let (_, run) = hooks
            .iter()
            .find(|(o, _)| *o == on)
            .expect("hook registered");
        run(input)
    }
}

impl RuleTarget for Recorder {
    fn where_value(&mut self, column: &str, op: FilterOp, value: CborValue) -> Result<String> {
        let described = format!("{column} {} {value:?}", op.key_symbol());
        self.calls
            .lock()
            .unwrap()
            .push(format!("where {described}"));
        Ok(described)
    }

    fn native_condition(
        &mut self,
        _condition: Box<dyn Any + Send>,
        preview: String,
    ) -> Result<String> {
        self.calls.lock().unwrap().push(format!("native {preview}"));
        Ok(preview)
    }

    fn hook(&mut self, on: HookOn, run: RuleHookFn) -> Result<()> {
        self.calls.lock().unwrap().push(format!("hook {on:?}"));
        self.hooks.lock().unwrap().push((on, run));
        Ok(())
    }

    fn remove_column(&mut self, column: &str) -> Result<()> {
        self.calls.lock().unwrap().push(format!("remove {column}"));
        Ok(())
    }
}

fn host() -> Host {
    Host::builder(Limits::background())
        .vocab_fn(register_rule_vocab)
        .build()
}

fn run(code: &str, env: Env) -> (Recorder, RuleHandle, Result<()>) {
    let recorder = Recorder::default();
    let handle = RuleHandle::new(Box::new(recorder.clone()));
    let outcome = eval_rule_script(&host(), code, env, &handle);
    (recorder, handle, outcome)
}

fn record(pairs: &[(&str, &str)]) -> RuleHookInput {
    RuleHookInput::Record(CborValue::Map(
        pairs
            .iter()
            .map(|(k, v)| {
                (
                    CborValue::Text(k.to_string()),
                    CborValue::Text(v.to_string()),
                )
            })
            .collect(),
    ))
}

#[test]
fn where_narrows_and_is_recorded() {
    let (recorder, handle, outcome) = run(
        r#"table.where("bakery_id", 3); table.where("price", ">", 10);"#,
        Env::new(),
    );
    outcome.unwrap();
    assert_eq!(
        recorder.calls(),
        [
            "where bakery_id = Integer(Integer(3))",
            "where price > Integer(Integer(10))"
        ]
    );
    assert_eq!(
        handle.verbs(),
        [
            RuleVerb::Where {
                column: "bakery_id".into(),
                described: "bakery_id = Integer(Integer(3))".into(),
            },
            RuleVerb::Where {
                column: "price".into(),
                described: "price > Integer(Integer(10))".into(),
            },
        ]
    );

    let (_, _, outcome) = run(r#"table.where("price", "between", 10)"#, Env::new());
    let message = outcome.unwrap_err().to_string();
    assert!(
        message.contains("unknown operator \"between\""),
        "{message}"
    );
}

#[test]
fn before_insert_closure_throws_its_message() {
    let (recorder, handle, outcome) = run(
        r#"table.before_insert(|record| throw "viewers change nothing");
           table.before_delete(|id| if id == "keep" { throw "kept" });"#,
        Env::new(),
    );
    outcome.unwrap();
    assert_eq!(
        handle.verbs(),
        [
            RuleVerb::Hook {
                on: HookOn::BeforeInsert
            },
            RuleVerb::Hook {
                on: HookOn::BeforeDelete
            },
        ]
    );
    assert_eq!(
        recorder.fire(HookOn::BeforeInsert, record(&[("name", "a")])),
        Err("viewers change nothing".to_string())
    );
    assert_eq!(
        recorder.fire(
            HookOn::BeforeDelete,
            RuleHookInput::Id(CborValue::Text("keep".into()))
        ),
        Err("kept".to_string())
    );
    assert_eq!(
        recorder.fire(
            HookOn::BeforeDelete,
            RuleHookInput::Id(CborValue::Text("other".into()))
        ),
        Ok(())
    );
}

#[test]
fn closure_captures_script_variables() {
    let mut session = RhaiMap::new();
    session.insert("role".into(), "editor".into());
    session.insert("envs".into(), "dev,staging".into());
    let (recorder, _, outcome) = run(
        r#"
        if session.role == "editor" {
            let mine = session.envs.split(",");
            table.before_update(|record| if !mine.contains(record.environment) { throw "editors change only their environments" });
            table.before_save(|record| if record.environment == "prod" { throw "no prod" });
        }
        "#,
        Env::new().var("session", session),
    );
    outcome.unwrap();
    assert_eq!(
        recorder.fire(HookOn::BeforeUpdate, record(&[("environment", "dev")])),
        Ok(())
    );
    assert_eq!(
        recorder.fire(HookOn::BeforeUpdate, record(&[("environment", "prod")])),
        Err("editors change only their environments".to_string())
    );
    assert_eq!(
        recorder.fire(HookOn::BeforeSave, record(&[("environment", "prod")])),
        Err("no prod".to_string())
    );
}

#[test]
fn remove_column_reaches_the_table() {
    let (recorder, handle, outcome) = run(
        r#"table.remove_column("bakery_id"); table.remove_column("bakery_name");"#,
        Env::new(),
    );
    outcome.unwrap();
    assert_eq!(recorder.calls(), ["remove bakery_id", "remove bakery_name"]);
    assert_eq!(
        handle.verbs(),
        [
            RuleVerb::RemoveColumn {
                column: "bakery_id".into()
            },
            RuleVerb::RemoveColumn {
                column: "bakery_name".into()
            },
        ]
    );
}

#[test]
fn the_handle_has_no_read_or_table_verbs() {
    for code in [
        "table.list()",
        r#"table("x")"#,
        r#"table.ref("y")"#,
        r#"table.get("1")"#,
        r#"table.insert(#{ name: "a" })"#,
        r#"table.delete("1")"#,
        r#"table.sort("name")"#,
        "table.limit(1)",
        r#"table.search("a")"#,
        // Only a backend that registers rule conditions has it.
        r#"table.with_condition("1 = 1")"#,
    ] {
        let (recorder, _, outcome) = run(code, Env::new());
        let message = outcome.unwrap_err().to_string();
        assert!(message.contains("Function not found"), "{code}: {message}");
        assert!(recorder.calls().is_empty(), "{code}");
    }
}
