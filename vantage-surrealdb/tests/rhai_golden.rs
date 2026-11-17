//! Rhai DSL golden suite: every `tests/rhai-tests/*.rhai` script against its
//! pinned `.surql` output or `.err` message. Offline: scripts are rendered,
//! never sent to a database. A golden that no longer matches the renderer
//! fails here.

#![cfg(feature = "rhai")]

#[path = "support/rhai_golden.rs"]
mod golden;

#[test]
fn every_fixture_matches_its_golden() {
    let mismatches = golden::mismatches();
    assert!(mismatches.is_empty(), "{}", mismatches.join("\n\n"));
}

#[test]
fn or_and_group_renders_parenthesised() {
    let code = r#"
        let a = surreal::ident("a");
        (a == 1).and_(surreal::ident("b") == 2).or_(surreal::ident("c") == 3)
    "#;
    let host = vantage_rhai::Host::builder(vantage_rhai::Limits::background())
        .vocab_fn(vantage_surrealdb::rhai_engine::register_surreal_onto)
        .build();
    let expr = host
        .compile(&vantage_rhai::Block::from(code))
        .and_then(|script| {
            script.eval_as::<vantage_surrealdb::rhai_engine::RhaiExpr>(&vantage_rhai::Env::new())
        })
        .unwrap();
    assert_eq!(expr.0.preview(), "((a = 1 AND b = 2) OR c = 3)");
}

/// The `surreal::` operators build expressions only from builder operands;
/// plain values compare as Rhai compares them.
#[test]
fn plain_values_still_compare_as_bools() {
    let host = vantage_rhai::Host::builder(vantage_rhai::Limits::background())
        .vocab_fn(vantage_surrealdb::rhai_engine::register_surreal_onto)
        .build();
    let code = r#"
        [
            "dev" == "dev", "dev" != "dev", 1 == 1.0, 2 > 1, 1.5 <= 1,
            "a" < "b", true == true, ["a", "b"].contains("a"), ["a"].contains("c"),
            () == "x", () != "x", () == (), "a" == 1, "a" != 1, [1, "a"] == [1, "a"]
        ]
    "#;
    let out = host
        .compile(&vantage_rhai::Block::from(code))
        .and_then(|script| script.eval(&vantage_rhai::Env::new()))
        .unwrap();
    let bools: Vec<bool> = out.into_typed_array::<bool>().expect("an array of bools");
    assert_eq!(
        bools,
        [
            true, false, true, true, false, true, true, true, false, false, true, true, false,
            true, true
        ]
    );
    let expr = host
        .compile(&vantage_rhai::Block::from(r#"surreal::ident("a") == "x""#))
        .and_then(|script| {
            script.eval_as::<vantage_surrealdb::rhai_engine::RhaiExpr>(&vantage_rhai::Env::new())
        })
        .unwrap();
    assert_eq!(expr.0.preview(), r#"a = "x""#);
}
