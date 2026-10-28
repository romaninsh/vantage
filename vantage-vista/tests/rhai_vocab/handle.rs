use ciborium::Value as CborValue;
use vantage_dataset::ReadableValueSet;
use vantage_vista::rhai::Handle;
use vantage_vista::{Condition, FilterOp, NativeCondition, SortDirection, VistaQuery};

use super::support::{read_write, resolver, run, store};

fn handle(script: &str) -> Handle {
    let host = read_write(&store());
    run(&host, script)
        .unwrap()
        .try_cast::<Handle>()
        .expect("a table handle")
}

fn int(n: i64) -> CborValue {
    CborValue::Integer(n.into())
}

#[test]
fn an_unnarrowed_handle_is_an_empty_query() {
    assert_eq!(handle(r#"table("t")"#).query().unwrap(), VistaQuery::new());
}

#[test]
fn where_steps_become_conditions_in_the_order_they_were_added() {
    let query = handle(r#"table("t").where("a", 1).where("n", ">", 2)"#)
        .query()
        .unwrap();
    assert_eq!(
        query.conditions,
        [
            Condition::new("a", FilterOp::Eq, int(1)),
            Condition::new("n", FilterOp::Gt, int(2)),
        ]
    );
}

#[test]
fn every_documented_operator_spelling_reads_as_its_operator() {
    let spellings = [
        ("=", FilterOp::Eq),
        ("==", FilterOp::Eq),
        ("eq", FilterOp::Eq),
        ("!=", FilterOp::Ne),
        ("ne", FilterOp::Ne),
        (">", FilterOp::Gt),
        ("gt", FilterOp::Gt),
        (">=", FilterOp::Gte),
        ("gte", FilterOp::Gte),
        ("<", FilterOp::Lt),
        ("lt", FilterOp::Lt),
        ("<=", FilterOp::Lte),
        ("lte", FilterOp::Lte),
        ("like", FilterOp::Like),
    ];
    for (spelling, op) in spellings {
        let query = handle(&format!(r#"table("t").where("a", "{spelling}", 1)"#))
            .query()
            .unwrap();
        assert_eq!(query.conditions[0].op, op, "{spelling}");
    }
    for (spelling, op) in [("in", FilterOp::InSet), ("not_in", FilterOp::NotInSet)] {
        let query = handle(&format!(r#"table("t").where("a", "{spelling}", [1, 2])"#))
            .query()
            .unwrap();
        assert_eq!(query.conditions[0].op, op, "{spelling}");
        assert_eq!(
            query.conditions[0].value,
            CborValue::Array(vec![int(1), int(2)])
        );
    }
}

#[test]
fn an_unknown_operator_is_a_script_error_naming_it() {
    let host = read_write(&store());
    let err = run(&host, r#"table("t").where("a", "approx", 1)"#).unwrap_err();
    assert!(err.contains("approx"), "{err}");
}

#[test]
fn search_and_sort_keep_the_last_one() {
    let query = handle(r#"table("t").search("a").search("b").sort("n").sort("a", "desc")"#)
        .query()
        .unwrap();
    assert_eq!(query.search.as_deref(), Some("b"));
    assert_eq!(
        query.order,
        Some(("a".to_string(), SortDirection::Descending))
    );
}

#[test]
fn a_chained_script_folds_into_one_query() {
    let query = handle(r#"table("t").where("a", 1).search("x").sort("n").where("n", 2)"#)
        .query()
        .unwrap();
    assert_eq!(query.conditions.len(), 2);
    assert_eq!(query.search.as_deref(), Some("x"));
    assert_eq!(
        query.order,
        Some(("n".to_string(), SortDirection::Ascending))
    );
}

#[tokio::test]
async fn the_query_reapplied_matches_the_resolved_handle() {
    let h = handle(r#"table("t").where("a", 1).sort("n", "desc")"#);
    let resolver = resolver(&store());
    let resolved = h.resolve(Some(resolver.as_ref())).unwrap();
    let (reapplied, residual) = resolver
        .vista("t")
        .unwrap()
        .with_query(&h.query().unwrap())
        .unwrap();
    assert!(residual.is_empty());
    assert_eq!(reapplied.query(), resolved.query());
    let a: Vec<String> = resolved.list_values().await.unwrap().into_keys().collect();
    let b: Vec<String> = reapplied.list_values().await.unwrap().into_keys().collect();
    assert_eq!(a, b);
}

#[test]
fn ref_limit_and_offset_have_no_query_form_and_the_error_names_the_verb() {
    for (script, verb) in [
        (r#"table("t").ref("orders")"#, "ref"),
        (r#"table("t").limit(2)"#, "limit"),
        (r#"table("t").offset(2)"#, "offset"),
    ] {
        let err = handle(script).query().unwrap_err();
        assert!(err.is_incorrect_usage(), "{err:?}");
        assert!(format!("{err:?} {err}").contains(verb), "{err:?}");
    }
}

#[test]
fn a_named_handle_knows_its_table_and_resolves_through_a_given_resolver() {
    let h = Handle::named("t");
    assert_eq!(h.table_name(), Some("t"));
    let resolver = resolver(&store());
    assert_eq!(h.resolve(Some(resolver.as_ref())).unwrap().name(), "t");
    assert!(h.resolve(None).is_err(), "no resolver, no table");
    let with = Handle::named("t").with_resolver(resolver);
    assert_eq!(with.resolve(None).unwrap().name(), "t");
}

#[test]
fn a_native_condition_needs_a_vista_in_hand() {
    let err = match Handle::named("t")
        .with_native_condition("with_condition", NativeCondition::new("x", "x"))
    {
        Ok(_) => panic!("a named handle holds no Vista to narrow"),
        Err(err) => err,
    };
    assert!(format!("{err:?} {err}").contains("with_condition"));
}
