use std::sync::Arc;

use ciborium::Value as CborValue;
use vantage_core::error;
use vantage_dataset::ReadableValueSet;
use vantage_rhai::rhai::{Array, Dynamic};
use vantage_rhai::{Block, Env, Host, Limits};
use vantage_types::Record;

use crate::rhai::bridge::block_on;
use crate::rhai::handle::{Handle, Step};
use crate::rhai::vocab::{DataVocab, Terminals};
use crate::{
    Column, ColumnFlag, FilterOp, NativeCondition, TableShell, Vista, VistaMetadata, VistaResolver,
    forward_table_shell, mocks::MockShell,
};

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
        .with_column(Column::new("id", "String").with_flag(ColumnFlag::Id))
        .with_column(Column::new("name", "String").with_flag(ColumnFlag::Title))
        .with_column(Column::new("vip_flag", "bool"))
        .with_id_column("id");
    Vista::new("users", source.with_metadata(metadata))
}

pub(super) fn mock_resolver() -> Arc<dyn VistaResolver> {
    Arc::new(|name: &str| {
        if name == "users" {
            Ok(users_vista())
        } else {
            Err(error!("unknown table in test resolver", table = name))
        }
    })
}

pub(super) fn describe_host(resolver: Arc<dyn VistaResolver>) -> Host {
    Host::builder(Limits::background())
        .vocab(DataVocab::new(Terminals::Describe).with_resolver(resolver))
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
    let v = h.resolve(Some(mock_resolver().as_ref())).unwrap();
    assert_eq!(block_on(v.list_values()).unwrap().unwrap().len(), 2);
}

/// Takes any native condition; the Vista records its key.
#[derive(Clone)]
struct Native {
    inner: Box<dyn TableShell>,
}

forward_table_shell!(Native, inner, {
    fn add_native_condition(&mut self, _condition: NativeCondition) -> vantage_core::Result<()> {
        Ok(())
    }
});

fn only_vip() -> NativeCondition {
    NativeCondition::new((), "vip_flag = true")
}

#[test]
fn with_native_condition_narrows_a_copy_of_the_base() {
    let base = Handle::over(users_vista().map_shell(|inner| Native { inner }));
    let narrowed = base.with_native_condition("only_vip", only_vip()).unwrap();
    let native = |h: &Handle| h.resolve(None).unwrap().query().native.clone();
    assert_eq!(native(&narrowed), ["vip_flag = true"]);
    assert!(
        native(&base).is_empty(),
        "the handle it came from keeps its base"
    );
}

#[test]
fn with_native_condition_needs_a_vista_in_hand() {
    let err = Handle::named("users")
        .with_native_condition("only_vip", only_vip())
        .err()
        .expect("a named handle has no Vista in hand");
    assert!(err.to_string().contains("only works on `self`"), "{err}");

    let past_ref = Handle::over(users_vista()).push(Step::Ref("posts".into()));
    let err = past_ref
        .with_native_condition("only_vip", only_vip())
        .err()
        .expect("a ref step leaves the Vista in hand");
    assert!(err.to_string().contains("can't follow `ref(...)`"), "{err}");
}

#[test]
fn a_refused_native_condition_is_an_error() {
    let err = Handle::over(users_vista())
        .with_native_condition("only_vip", only_vip())
        .err()
        .expect("the mock takes no native conditions");
    assert!(err.is_unsupported(), "{err}");
}
