use vantage_rhai::rhai::Engine;
use vantage_vista::rhai::reject_dropped_narrowing;

fn check(script: &str, has_value: bool) -> Result<(), String> {
    let ast = Engine::new().compile(script).unwrap();
    reject_dropped_narrowing(&ast, "test script", has_value).map_err(|e| format!("{e:?} {e}"))
}

fn refused(script: &str, has_value: bool) {
    let err = check(script, has_value).expect_err(script);
    assert!(err.contains("discarded"), "{script}: {err}");
}

fn passes(script: &str, has_value: bool) {
    check(script, has_value).unwrap_or_else(|e| panic!("{script}: {e}"));
}

#[test]
fn a_dropped_narrowing_is_refused_on_any_receiver() {
    for verb in [
        r#"where("a", 1)"#,
        r#"sort("n")"#,
        r#"search("x")"#,
        r#"limit(2)"#,
        r#"offset(2)"#,
        r#"ref("orders")"#,
        r#"with_condition("a", 1)"#,
    ] {
        refused(&format!("let open = table(); open.{verb}; 1"), false);
        refused(&format!("table(\"t\").{verb}; 1"), false);
        refused(&format!("self.{verb}; 1"), false);
        refused(&format!("table(\"t\").where(\"a\", 1).{verb}; 1"), false);
    }
}

#[test]
fn the_error_is_incorrect_usage_and_names_the_script() {
    let ast = Engine::new().compile("self.where(\"a\", 1); 1").unwrap();
    let err = reject_dropped_narrowing(&ast, "modify script", false).unwrap_err();
    assert!(err.is_incorrect_usage(), "{err:?}");
    assert!(format!("{err:?} {err}").contains("modify script"));
}

#[test]
fn the_statement_giving_the_value_may_narrow_only_when_the_value_is_used() {
    passes(r#"self.where("a", 1)"#, true);
    passes(r#"self.where("a", 1);"#, true);
    refused(r#"self.where("a", 1)"#, false);
    refused(r#"self.where("a", 1);"#, false);
    refused(r#"self.where("a", 1); self.sort("n");"#, true);
    passes(
        r#"if x { self.where("a", 1) } else { self.sort("n") }"#,
        true,
    );
    refused(
        r#"if x { self.where("a", 1) } else { self.sort("n") }"#,
        false,
    );
    passes(r#"{ self.where("a", 1) }"#, true);
    refused(r#"{ self.where("a", 1); 1 }"#, true);
}

#[test]
fn bodies_of_control_flow_are_checked_and_loops_have_no_value_position() {
    refused(r#"{ self.where("a", 1); 1 } 2"#, true);
    refused(r#"let x = 1; { self.where("a", 1); } x"#, true);
    refused(r#"if x { self.where("a", 1); 1 } else { 2 }"#, true);
    refused(r#"if x { 1 } else { self.where("a", 1); 2 }"#, true);
    refused(r#"while x { self.where("a", 1) }"#, true);
    refused(r#"for i in 0..3 { self.where("a", 1) }"#, true);
    refused(r#"do { self.where("a", 1) } while x;"#, true);
}

#[test]
fn a_try_body_is_checked() {
    refused(r#"try { self.where("a", 1); 1 } catch (e) { 2 }"#, true);
    refused(r#"try { self.where("a", 1); 1 } catch (e) { 2 } 3"#, false);
}

#[test]
fn a_catch_body_is_checked() {
    // A try body that can't throw compiles away its catch, so this one throws.
    refused(
        r#"try { throw 1; } catch (e) { self.where("a", 1); 2 }"#,
        true,
    );
}

#[test]
fn terminals_writes_and_kept_handles_pass() {
    passes(r#"table("t").insert(#{ a: 1 }); 1"#, false);
    passes(r#"table("t").list(); 1"#, false);
    passes(r#"let open = self.where("a", 1); open.list(); 1"#, false);
    passes(r#"let x = self.where("a", 1).sort("n"); x"#, false);
    passes(r#"foo(self.where("a", 1)); 1"#, false);
}

#[test]
fn rhais_own_array_sort_is_not_a_narrowing() {
    passes("ids.sort(); 1", false);
    passes("ids.sort(|a, b| a - b); 1", false);
    passes(r#"ids.sort(Fn("cmp")); 1"#, false);
    refused("ids.sort(cmp); 1", false);
    refused(r#"table("t").sort("n", "desc"); 1"#, false);
    passes("let x = 1; ids.sort(|a, b| a - x); 1", false);
}

#[test]
fn fn_definitions_and_switch_arms_are_not_descended_into() {
    passes(r#"fn f(h) { h.where("a", 1); 1 } 1"#, false);
    passes(
        r#"switch x { 1 => { self.where("a", 1); } _ => () } 1"#,
        false,
    );
}
