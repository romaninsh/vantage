//! `reject_dropped_narrowing` called directly. The entry points that apply
//! it to handle scripts are covered in `rhai::tests::eval`.

use vantage_rhai::rhai::Engine;

use super::reject_dropped_narrowing;

fn check(script: &str, has_value: bool) -> vantage_core::Result<()> {
    let ast = Engine::new().compile(script).expect("the script compiles");
    reject_dropped_narrowing(&ast, "test script", has_value)
}

#[test]
fn a_dropped_narrowing_is_refused_on_any_receiver() {
    for script in [
        r#"table().where("k", 1); table().ids()"#,
        r#"let t = table(); t.where("k", 1); t.ids()"#,
        r#"let t = table(); t.sort("n").limit(2); t"#,
        r#"let t = table(); t.sort("name"); t.ids()"#,
        r#"let t = table(); t.sort("name", "desc"); t"#,
        r#"for i in 0..2 { table().ref("orders"); }"#,
        r#"let x = self; x.where("a", 1); x"#,
        r#"let x = self; x.with_condition(c); x"#,
    ] {
        for has_value in [false, true] {
            let err = check(script, has_value).expect_err(script);
            assert!(err.is_incorrect_usage(), "{script}: {err}");
            assert!(err.to_string().contains("discarded"), "{script}: {err}");
        }
    }
}

/// The statement giving a script its value is in value position only when
/// the value is used.
#[test]
fn the_value_statement_may_narrow_only_when_the_value_is_used() {
    for script in [
        r#"table().where("k", 1)"#,
        r#"if true { table("x").search("a") }"#,
        r#"self.where("a", 1);"#,
        r#"if row.a > 1 { self.where("a", 1) } else { self }"#,
    ] {
        check(script, true).unwrap_or_else(|e| panic!("{script}: {e}"));
        let err = check(script, false).expect_err(script);
        assert!(err.to_string().contains("discarded"), "{script}: {err}");
    }
}

#[test]
fn terminals_writes_and_kept_handles_pass() {
    for script in [
        r#"table().insert(#{ id: "a" }); table().where("k", 1).patch("a", #{ n: 1 });"#,
        r#"let open = table().where("status", "open"); open.ids()"#,
        r#"for id in table().where("k", 1).ids() { table().delete(id); }"#,
        r#"let n = table().sort("n").limit(2).list().len();"#,
        r#"let a = table().ids(); a.sort(); a"#,
        r#"let a = [3, 1, 2]; a.sort(|x, y| x - y);"#,
        r#"let k = 1; let a = [3, 1, 2]; a.sort(|x, y| x - y + k - k);"#,
        r#"let a = [3, 1, 2]; a.sort(Fn("cmp")); fn cmp(x, y) { x - y }"#,
        r#"let all = self; let mine = all.where("a", 1); all"#,
    ] {
        for has_value in [false, true] {
            check(script, has_value).unwrap_or_else(|e| panic!("{script}: {e}"));
        }
    }
}
