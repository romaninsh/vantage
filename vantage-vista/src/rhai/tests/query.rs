use ciborium::Value as CborValue;
use vantage_rhai::Host;

use super::narrow::{Native, describe_host, eval, mock_resolver, only_vip, users_vista};
use crate::rhai::handle::Handle;
use crate::{Condition, FilterOp, SortDirection, VistaQuery};

fn host() -> Host {
    describe_host(mock_resolver())
}

fn query_of(script: &str) -> vantage_core::Result<VistaQuery> {
    eval::<Handle>(&host(), script).query()
}

#[test]
fn an_unnarrowed_handle_is_an_empty_query() {
    assert!(query_of(r#"table("users")"#).unwrap().is_empty());
}

#[test]
fn where_steps_become_conditions_in_order() {
    let q = query_of(r#"table("users").where("vip_flag", true).where("age", ">=", 18)"#).unwrap();
    assert_eq!(
        q.conditions,
        [
            Condition::new("vip_flag", FilterOp::Eq, true),
            Condition::new("age", FilterOp::Gte, CborValue::Integer(18.into())),
        ]
    );
}

#[test]
fn search_and_sort_keep_the_last_one() {
    let q =
        query_of(r#"table("users").search("al").sort("name").search("car").sort("id", "desc")"#)
            .unwrap();
    assert_eq!(q.search.as_deref(), Some("car"));
    assert_eq!(q.order, Some(("id".to_string(), SortDirection::Descending)));
}

#[test]
fn a_chained_script_folds_into_one_query() {
    let q = query_of(
        r#"let vips = table("users").where("vip_flag", true);
           let named = vips.search("a");
           named.sort("name").where("name", "!=", "Bob")"#,
    )
    .unwrap();
    let expected = VistaQuery::new()
        .with_condition(Condition::new("vip_flag", FilterOp::Eq, true))
        .with_condition(Condition::new("name", FilterOp::Ne, "Bob"))
        .with_search("a")
        .with_order("name", SortDirection::Ascending);
    assert_eq!(q, expected);
}

#[test]
fn the_query_reapplied_matches_the_resolved_handle() {
    let handle: Handle = eval(
        &host(),
        r#"table("users").where("vip_flag", true).search("a")"#,
    );
    let resolved = handle.resolve(Some(mock_resolver().as_ref())).unwrap();
    assert_eq!(resolved.query(), &handle.query().unwrap());
}

fn assert_refused(err: vantage_core::VantageError, verb: &str) {
    assert!(err.is_incorrect_usage(), "{err}");
    let named = format!("verb: \"{verb}\"");
    assert!(err.to_string().contains(&named), "names `{verb}`: {err}");
}

#[test]
fn ref_and_limit_are_refused_naming_the_verb() {
    let err = query_of(r#"table("users").where("vip_flag", true).ref("orders")"#).unwrap_err();
    assert_refused(err, "ref");
    let err = query_of(r#"table("users").limit(5)"#).unwrap_err();
    assert_refused(err, "limit");
}

#[test]
fn a_native_condition_is_refused_naming_the_verb() {
    let base = Handle::over(users_vista().map_shell(|inner| Native { inner }));
    let narrowed = base.with_native_condition("only_vip", only_vip()).unwrap();
    assert_refused(narrowed.query().unwrap_err(), "only_vip");
    assert!(
        base.query().unwrap().is_empty(),
        "the base handle is unchanged"
    );
}
