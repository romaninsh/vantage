use std::sync::Arc;

use ciborium::Value as CborValue;
use vantage_core::error;
use vantage_dataset::ReadableValueSet;
use vantage_rhai::rhai::{Array, Dynamic};
use vantage_rhai::{Block, Env, Host, Limits};
use vantage_types::Record;

use crate::rhai::bridge::block_on;
use crate::rhai::handle::{Handle, Step};
use crate::rhai::vocab::{DataVocab, TargetResolver};
use crate::{Column, FilterOp, Vista, VistaMetadata, mocks::MockShell};

pub(super) fn user(id: &str, name: &str, vip: bool) -> Record<CborValue> {
    [
        ("id", CborValue::Text(id.into())),
        ("name", CborValue::Text(name.into())),
        ("vip_flag", CborValue::Bool(vip)),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v))
    .collect()
}

/// Fresh `users` Vista: Alice and Carol are VIPs, Bob is not.
pub(super) fn users_vista() -> Vista {
    let source = MockShell::new()
        .with_record("1", user("1", "Alice", true))
        .with_record("2", user("2", "Bob", false))
        .with_record("3", user("3", "Carol", true));
    let metadata = VistaMetadata::new()
        .with_column(Column::new("id", "String").with_flag("id"))
        .with_column(Column::new("name", "String").with_flag("title"))
        .with_column(Column::new("vip_flag", "bool"))
        .with_id_column("id");
    Vista::new("users", Box::new(source.with_metadata(metadata)))
}

pub(super) fn mock_resolver() -> TargetResolver {
    Arc::new(|name: &str| {
        if name == "users" {
            Ok(users_vista())
        } else {
            Err(error!("unknown table in test resolver", table = name))
        }
    })
}

pub(super) fn describe_host(resolver: TargetResolver) -> Host {
    Host::builder(Limits::background())
        .vocab(DataVocab::describe(Some(resolver)))
        .build()
}

fn run(host: &Host, script: &str) -> Result<Dynamic, String> {
    host.compile_uncached(&Block::from(script))
        .and_then(|s| s.eval(&Env::new()))
        .map_err(|e| e.to_string())
}

fn eval<T: Clone + 'static>(host: &Host, script: &str) -> T {
    run(host, script)
        .unwrap()
        .try_cast::<T>()
        .expect("unexpected result type")
}

fn eval2<A: Clone + 'static, B: Clone + 'static>(host: &Host, script: &str) -> (A, B) {
    let mut items = eval::<Array>(host, script).into_iter();
    let a = items.next().unwrap().try_cast::<A>().unwrap();
    let b = items.next().unwrap().try_cast::<B>().unwrap();
    (a, b)
}

fn eval_err(host: &Host, script: &str) -> String {
    run(host, script).expect_err("script should fail")
}

#[test]
fn stored_handle_is_not_changed_by_later_chain() {
    let host = describe_host(mock_resolver());
    let (a, b): (Handle, Handle) = eval2(
        &host,
        r#"let o = table("users").where("vip_flag", true); [o.sort("name"), o]"#,
    );
    assert_eq!(a.steps().len(), 2);
    assert_eq!(b.steps().len(), 1);
}

#[test]
fn where_with_operator_parses_filter_op() {
    let h: Handle = eval(
        &describe_host(mock_resolver()),
        r#"table("users").where("age", ">=", 18)"#,
    );
    assert!(matches!(
        h.steps()[0],
        Step::Where {
            op: FilterOp::Gte,
            ..
        }
    ));
}

#[test]
fn unknown_operator_is_a_script_error_naming_it() {
    let err = eval_err(
        &describe_host(mock_resolver()),
        r#"table("users").where("age", "~~", 1)"#,
    );
    assert!(err.contains("~~"), "{err}");
}

#[test]
fn resolve_applies_steps_in_order() {
    let h = Handle::named("users").push(Step::Where {
        col: "vip_flag".into(),
        op: FilterOp::Eq,
        value: CborValue::Bool(true),
    });
    let v = h.resolve(Some(&mock_resolver())).unwrap();
    assert_eq!(block_on(v.list_values()).unwrap().unwrap().len(), 2);
}
