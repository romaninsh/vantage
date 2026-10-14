use vantage_dataset::ReadableValueSet;
use vantage_rhai::Env;
use vantage_vista::rhai::{Terminals, block_on, eval_modify_script, eval_ref_script};
use vantage_vista::{Condition, FilterOp, NativeCondition, TableShell, Vista, forward_table_shell};

use super::support::{host, rec, resolver, store};

fn ids(vista: &Vista) -> Vec<String> {
    block_on(vista.list_values())
        .unwrap()
        .unwrap()
        .into_keys()
        .collect()
}

#[test]
fn modify_script_narrows_self() {
    let store = store();
    let host = host(&store, Terminals::Describe);
    let base = resolver(&store).vista("t").unwrap();
    let vista = eval_modify_script(&host, r#"self.where("a", 1).sort("n")"#, base).unwrap();
    assert_eq!(ids(&vista), ["r3", "r1"]);
}

/// The Vista `t` over `store`, taking native conditions: each is a
/// [`Condition`] the inner shell applies.
fn base(store: &vantage_memory::MemoryStore) -> Vista {
    resolver(store)
        .vista("t")
        .unwrap()
        .map_shell(|inner| NativeEq { inner })
}

#[derive(Clone)]
struct NativeEq {
    inner: Box<dyn TableShell>,
}

forward_table_shell!(NativeEq, inner, {
    fn add_native_condition(&mut self, condition: NativeCondition) -> vantage_core::Result<()> {
        let condition: Condition = condition
            .downcast()
            .map_err(|_| vantage_core::error!("not a Condition"))?;
        self.inner.add_condition(&condition)
    }
});

/// A Describe host over `store` with the extension verb
/// `with_condition(col, value)`, an equality passed as a native condition.
fn host_with_extension(store: &vantage_memory::MemoryStore) -> vantage_rhai::Host {
    use vantage_rhai::rhai::{Dynamic, EvalAltResult};
    use vantage_vista::rhai::{DataVocab, Handle, dynamic_to_cbor};
    vantage_rhai::Host::builder(vantage_rhai::Limits::background())
        .vocab_fn(|engine| {
            engine.register_fn(
                "with_condition",
                |h: &mut Handle, col: &str, value: Dynamic| -> Result<Handle, Box<EvalAltResult>> {
                    let key = format!("{col} = {value}");
                    let condition = Condition::new(col, FilterOp::Eq, dynamic_to_cbor(value)?);
                    h.with_native_condition("with_condition", NativeCondition::new(condition, key))
                        .map_err(|e| e.to_string().into())
                },
            );
        })
        .vocab(DataVocab::new(Terminals::Describe).with_resolver(resolver(store)))
        .build()
}

#[test]
fn a_reference_script_that_does_not_compile_is_an_error() {
    let store = store();
    let host = host(&store, Terminals::Describe);
    let err = eval_ref_script(&host, "let x = ;", Env::new(), &rec(&[("a", 1)])).unwrap_err();
    assert!(!err.to_string().is_empty());
    let ok = eval_ref_script(
        &host,
        r#"table("t").where("a", row.a)"#,
        Env::new(),
        &rec(&[("a", 1)]),
    )
    .unwrap();
    assert!(ok.is_narrowed());
}

#[test]
fn an_extension_verb_is_refused_by_a_vista_that_takes_no_native_conditions() {
    let store = store();
    let host = host_with_extension(&store);
    let plain = resolver(&store).vista("t").unwrap();
    let err = eval_modify_script(&host, r#"self.with_condition("a", 1)"#, plain).unwrap_err();
    assert!(
        format!("{err:?} {err}").contains("with_condition")
            || err.is_unsupported()
            || err.is_unimplemented(),
        "{err:?}"
    );
}

#[test]
fn an_extension_verb_past_a_ref_or_on_a_named_table_is_an_error_naming_the_verb() {
    let store = store();
    let host = host_with_extension(&store);
    let err = eval_modify_script(&host, r#"table("t").with_condition("a", 1)"#, base(&store))
        .unwrap_err();
    assert!(
        format!("{err:?} {err}").contains("with_condition"),
        "{err:?}"
    );
    let err = eval_modify_script(
        &host,
        r#"self.ref("orders").with_condition("a", 1)"#,
        base(&store),
    )
    .unwrap_err();
    assert!(
        format!("{err:?} {err}").contains("with_condition"),
        "{err:?}"
    );
}

#[test]
fn a_script_not_ending_on_a_handle_is_an_error() {
    let store = store();
    let host = host_with_extension(&store);
    for script in [
        r#"if false { self.with_condition("a", 1) }"#,
        "let done = true;",
    ] {
        let err = eval_modify_script(&host, script, base(&store)).unwrap_err();
        assert!(
            err.to_string().contains("must end on a table handle"),
            "`{script}`: {err}"
        );
    }
    let code = r#"if row.a > 5 { table("t").where("a", row.a) }"#;
    let err = eval_ref_script(&host, code, Env::new(), &rec(&[("a", 1)])).unwrap_err();
    assert!(
        err.to_string().contains("must end on a table handle"),
        "{err}"
    );
}

#[test]
fn a_dropped_handle_is_refused() {
    let store = store();
    let host = host_with_extension(&store);
    let modified =
        eval_modify_script(&host, r#"self.with_condition("a", 1);"#, base(&store)).unwrap();
    assert_eq!(ids(&modified), ["r1", "r3"], "a trailing `;` is fine");
    for script in [
        r#"self.with_condition("a", 1); self.sort("n");"#,
        r#"self.with_condition("a", 1); let done = true;"#,
        r#"let all = self; all.with_condition("a", 1); all"#,
    ] {
        let err = eval_modify_script(&host, script, base(&store)).unwrap_err();
        assert!(
            err.to_string().contains("table handle is discarded"),
            "`{script}`: {err}"
        );
    }
}
