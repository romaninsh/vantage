use std::cmp::Ordering;

use ciborium::Value as CborValue;
use vantage_vista::eval::{cbor_cmp, cell_get};
use vantage_vista::{
    Column, ColumnFlag, Condition, FilterOp, SortDirection, VistaMetadata, VistaQuery,
};

use super::{i, rec, t};

fn cond(column: &str, op: FilterOp, value: CborValue) -> Condition {
    Condition::new(column, op, value)
}

fn nested(root: &str, leaf: &str, value: CborValue) -> CborValue {
    let _ = root;
    CborValue::Map(vec![(t(leaf), value)])
}

#[test]
fn every_operator_reads_a_flat_cell() {
    let meta = VistaMetadata::new();
    let row = rec(&[("n", i(5)), ("s", t("Hello"))]);
    let yes = |c: Condition| assert!(c.matches(&row, &meta), "{c:?}");
    let no = |c: Condition| assert!(!c.matches(&row, &meta), "{c:?}");
    yes(cond("n", FilterOp::Eq, i(5)));
    no(cond("n", FilterOp::Eq, i(6)));
    yes(cond("n", FilterOp::Ne, i(6)));
    no(cond("n", FilterOp::Ne, i(5)));
    yes(cond("n", FilterOp::Gt, i(4)));
    no(cond("n", FilterOp::Gt, i(5)));
    yes(cond("n", FilterOp::Gte, i(5)));
    no(cond("n", FilterOp::Gte, i(6)));
    yes(cond("n", FilterOp::Lt, i(6)));
    no(cond("n", FilterOp::Lt, i(5)));
    yes(cond("n", FilterOp::Lte, i(5)));
    no(cond("n", FilterOp::Lte, i(4)));
    yes(cond(
        "n",
        FilterOp::InSet,
        CborValue::Array(vec![i(1), i(5)]),
    ));
    no(cond(
        "n",
        FilterOp::InSet,
        CborValue::Array(vec![i(1), i(2)]),
    ));
    yes(cond("n", FilterOp::NotInSet, CborValue::Array(vec![i(1)])));
    no(cond("n", FilterOp::NotInSet, CborValue::Array(vec![i(5)])));
    yes(cond("s", FilterOp::Like, t("ELL")));
    no(cond("s", FilterOp::Like, t("xyz")));
}

#[test]
fn an_integer_equals_the_float_of_the_same_number() {
    let meta = VistaMetadata::new();
    let row = rec(&[("n", i(3)), ("f", CborValue::Float(3.0))]);
    assert!(cond("n", FilterOp::Eq, CborValue::Float(3.0)).matches(&row, &meta));
    assert!(cond("f", FilterOp::Eq, i(3)).matches(&row, &meta));
    assert!(
        cond(
            "n",
            FilterOp::InSet,
            CborValue::Array(vec![CborValue::Float(3.0)])
        )
        .matches(&row, &meta)
    );
    assert!(cond("n", FilterOp::Lt, CborValue::Float(4.95)).matches(&row, &meta));
}

#[test]
fn negative_zero_equals_zero() {
    assert_eq!(cbor_cmp(&CborValue::Float(-0.0), &i(0)), Ordering::Equal);
    assert_eq!(
        cbor_cmp(&CborValue::Float(-0.0), &CborValue::Float(0.0)),
        Ordering::Equal
    );
}

#[test]
fn nan_is_ordered_not_refused() {
    let nan = CborValue::Float(f64::NAN);
    assert_eq!(cbor_cmp(&nan, &nan), Ordering::Equal);
    assert_eq!(cbor_cmp(&nan, &i(1_000_000)), Ordering::Greater);
    assert_eq!(
        cbor_cmp(&CborValue::Float(-f64::NAN), &i(-1_000_000)),
        Ordering::Less
    );
}

#[test]
fn text_compares_by_code_point_and_false_precedes_true() {
    assert_eq!(cbor_cmp(&t("a"), &t("b")), Ordering::Less);
    assert_eq!(cbor_cmp(&t("b"), &t("a")), Ordering::Greater);
    assert_eq!(
        cbor_cmp(&CborValue::Bool(false), &CborValue::Bool(true)),
        Ordering::Less
    );
    assert_eq!(cbor_cmp(&i(3), &CborValue::Float(4.95)), Ordering::Less);
}

#[test]
fn a_datetime_is_taken_as_its_text() {
    let at = |s: &str| CborValue::Tag(0, Box::new(t(s)));
    assert_eq!(
        cbor_cmp(&at("2026-01-01T00:00:00Z"), &t("2026-01-01T00:00:00Z")),
        Ordering::Equal
    );
    assert_eq!(
        cbor_cmp(&at("2026-01-01T00:00:00Z"), &at("2026-02-01T00:00:00Z")),
        Ordering::Less
    );
    let meta = VistaMetadata::new();
    let row = rec(&[("when", at("2026-01-01T00:00:00Z"))]);
    assert!(cond("when", FilterOp::Eq, t("2026-01-01T00:00:00Z")).matches(&row, &meta));
}

#[test]
fn mixed_types_compare_deterministically() {
    let pairs = [
        (t("a"), i(1)),
        (CborValue::Bool(true), t("a")),
        (CborValue::Null, i(1)),
        (CborValue::Array(vec![i(1)]), CborValue::Map(vec![])),
    ];
    for (a, b) in pairs {
        let forward = cbor_cmp(&a, &b);
        assert_ne!(forward, Ordering::Equal, "{a:?} vs {b:?}");
        assert_eq!(cbor_cmp(&b, &a), forward.reverse());
    }
}

#[test]
fn an_absent_cell_matches_only_the_negative_operators() {
    let meta = VistaMetadata::new();
    let row = rec(&[("other", i(1))]);
    let set = CborValue::Array(vec![i(1)]);
    assert!(cond("n", FilterOp::Ne, i(1)).matches(&row, &meta));
    assert!(cond("n", FilterOp::NotInSet, set.clone()).matches(&row, &meta));
    for op in [
        FilterOp::Eq,
        FilterOp::Gt,
        FilterOp::Gte,
        FilterOp::Lt,
        FilterOp::Lte,
        FilterOp::Like,
    ] {
        assert!(!cond("n", op, i(1)).matches(&row, &meta));
    }
    assert!(!cond("n", FilterOp::InSet, set).matches(&row, &meta));
}

#[test]
fn a_dotted_column_reads_into_nested_maps() {
    let meta = VistaMetadata::new();
    let row = rec(&[("dims", nested("dims", "w", i(7)))]);
    assert_eq!(cell_get(&row, "dims.w", &meta), Some(&i(7)));
    assert_eq!(cell_get(&row, "dims.h", &meta), None);
    assert_eq!(
        cell_get(&row, "dims", &meta),
        Some(&nested("dims", "w", i(7)))
    );
    assert!(cond("dims.w", FilterOp::Gt, i(6)).matches(&row, &meta));
    assert_eq!(cell_get(&row, "dims.w.deeper", &meta), None);
    assert_eq!(cell_get(&row, "nope.w", &meta), None);
}

#[test]
fn a_literal_dotted_key_is_read_only_when_declared() {
    let row = rec(&[("author.name", t("Ann"))]);
    assert_eq!(cell_get(&row, "author.name", &VistaMetadata::new()), None);
    let meta = VistaMetadata::new().with_column(Column::new("author.name", "String"));
    assert_eq!(cell_get(&row, "author.name", &meta), Some(&t("Ann")));
}

#[test]
fn a_query_matches_when_every_condition_and_the_search_do() {
    let meta = VistaMetadata::new();
    let row = rec(&[("a", i(1)), ("name", t("Apple"))]);
    let query = VistaQuery::new()
        .with_condition(cond("a", FilterOp::Eq, i(1)))
        .with_search("app");
    assert!(query.is_locally_evaluable());
    assert!(query.matches(&row, &meta).unwrap());

    let miss = query.clone().with_condition(cond("a", FilterOp::Eq, i(2)));
    assert!(!miss.matches(&row, &meta).unwrap());

    let no_search = VistaQuery::new().with_search("zzz");
    assert!(!no_search.matches(&row, &meta).unwrap());
    assert!(VistaQuery::new().matches(&row, &meta).unwrap());
}

#[test]
fn a_query_holding_a_native_condition_is_not_evaluated() {
    let mut query = VistaQuery::new();
    query.native.push("raw".into());
    assert!(!query.is_locally_evaluable());
    let err = query
        .matches(&rec(&[("a", i(1))]), &VistaMetadata::new())
        .unwrap_err();
    assert!(err.is_incorrect_usage(), "{err:?}");
}

fn searchable_meta() -> VistaMetadata {
    VistaMetadata::new()
        .with_column(Column::new("name", "String").with_flag(ColumnFlag::Searchable))
        .with_column(Column::new("note", "String"))
}

#[test]
fn search_is_a_case_insensitive_substring() {
    let meta = VistaMetadata::new();
    let row = rec(&[("name", t("Delorean"))]);
    assert!(
        VistaQuery::new()
            .with_search("LOR")
            .matches(&row, &meta)
            .unwrap()
    );
    assert!(
        !VistaQuery::new()
            .with_search("flux")
            .matches(&row, &meta)
            .unwrap()
    );
    assert!(
        VistaQuery::new()
            .with_search("")
            .matches(&row, &meta)
            .unwrap()
    );
}

#[test]
fn search_reads_numbers_bools_tags_and_nested_values_as_text() {
    let meta = VistaMetadata::new();
    let row = rec(&[
        ("n", i(1985)),
        ("f", CborValue::Float(2.5)),
        ("b", CborValue::Bool(true)),
        (
            "id",
            CborValue::Tag(
                8,
                Box::new(CborValue::Array(vec![t("client"), t("bristol")])),
            ),
        ),
        (
            "deep",
            CborValue::Map(vec![(
                t("list"),
                CborValue::Array(vec![CborValue::Map(vec![(t("k"), t("hidden"))])]),
            )]),
        ),
        ("nothing", CborValue::Null),
        ("raw", CborValue::Bytes(b"secret".to_vec())),
    ]);
    let found = |needle: &str| {
        VistaQuery::new()
            .with_search(needle)
            .matches(&row, &meta)
            .unwrap()
    };
    assert!(found("1985"));
    assert!(found("2.5"));
    assert!(found("true"));
    assert!(found("bristol"));
    assert!(found("hidden"));
    assert!(!found("null"));
    assert!(!found("secret"));
    assert!(!found("list"));
}

#[test]
fn search_reads_only_searchable_columns_when_some_are_flagged() {
    let meta = searchable_meta();
    let row = rec(&[("name", t("apple")), ("note", t("secret"))]);
    assert!(
        VistaQuery::new()
            .with_search("app")
            .matches(&row, &meta)
            .unwrap()
    );
    assert!(
        !VistaQuery::new()
            .with_search("secret")
            .matches(&row, &meta)
            .unwrap()
    );

    let none_flagged = VistaMetadata::new()
        .with_column(Column::new("name", "String"))
        .with_column(Column::new("note", "String"));
    assert!(
        VistaQuery::new()
            .with_search("secret")
            .matches(&row, &none_flagged)
            .unwrap()
    );
}

fn people() -> Vec<(String, vantage_types::Record<CborValue>)> {
    vec![
        ("p1".into(), rec(&[("n", i(2))])),
        ("p2".into(), rec(&[("n", CborValue::Null)])),
        ("p3".into(), rec(&[("n", i(1))])),
        ("p4".into(), rec(&[])),
        ("p5".into(), rec(&[("n", i(2))])),
    ]
}

fn ids(rows: &[(String, vantage_types::Record<CborValue>)]) -> Vec<&str> {
    rows.iter().map(|(id, _)| id.as_str()).collect()
}

#[test]
fn absent_and_null_cells_sort_last_both_ways_and_ties_break_on_id() {
    let meta = VistaMetadata::new();
    let mut rows = people();
    VistaQuery::new()
        .with_order("n", SortDirection::Ascending)
        .sort_rows(&mut rows, &meta);
    assert_eq!(ids(&rows), ["p3", "p1", "p5", "p2", "p4"]);

    let mut rows = people();
    VistaQuery::new()
        .with_order("n", SortDirection::Descending)
        .sort_rows(&mut rows, &meta);
    assert_eq!(ids(&rows), ["p1", "p5", "p3", "p2", "p4"]);
}

#[test]
fn without_an_order_the_rows_keep_their_order() {
    let mut rows = people();
    VistaQuery::new().sort_rows(&mut rows, &VistaMetadata::new());
    assert_eq!(ids(&rows), ["p1", "p2", "p3", "p4", "p5"]);
}

#[test]
fn cmp_rows_agrees_with_sort_rows() {
    let meta = VistaMetadata::new();
    let query = VistaQuery::new().with_order("n", SortDirection::Ascending);
    let mut rows = people();
    query.sort_rows(&mut rows, &meta);
    for pair in rows.windows(2) {
        let (a, b) = (&pair[0], &pair[1]);
        assert_eq!(
            query.cmp_rows((&a.0, &a.1), (&b.0, &b.1), &meta),
            Ordering::Less
        );
    }
    let first = &rows[0];
    assert_eq!(
        query.cmp_rows((&first.0, &first.1), (&first.0, &first.1), &meta),
        Ordering::Equal
    );
    let unordered = VistaQuery::new();
    assert_eq!(
        unordered.cmp_rows(("a", &rec(&[])), ("b", &rec(&[])), &meta),
        Ordering::Less
    );
}

#[test]
fn a_dotted_order_column_walks_the_path() {
    let meta = VistaMetadata::new();
    let mut rows = vec![
        ("a".to_string(), rec(&[("dims", nested("dims", "w", i(9)))])),
        ("b".to_string(), rec(&[("dims", nested("dims", "w", i(3)))])),
    ];
    VistaQuery::new()
        .with_order("dims.w", SortDirection::Ascending)
        .sort_rows(&mut rows, &meta);
    assert_eq!(ids(&rows), ["b", "a"]);
}
