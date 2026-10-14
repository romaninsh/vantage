//! The query pipeline: conditions, search, ordering and windows, run through
//! `MemoryTable::query`.

use std::cmp::Ordering;

use ciborium::Value as CborValue;
use vantage_memory::eval::compare::{cmp_values, like, lookup, text_of, values_eq};
use vantage_memory::{MemoryCondition, MemoryStore, MemoryTable, MemoryTableHandle, Query};
use vantage_types::Record;
use vantage_vista::{FilterOp, SortDirection};

type Cells = Vec<(&'static str, CborValue)>;

fn text(s: &str) -> CborValue {
    CborValue::Text(s.into())
}
fn int(i: i64) -> CborValue {
    CborValue::Integer(i.into())
}
fn float(f: f64) -> CborValue {
    CborValue::Float(f)
}
fn array(items: Vec<CborValue>) -> CborValue {
    CborValue::Array(items)
}
fn record(cells: &[(&str, CborValue)]) -> Record<CborValue> {
    cells
        .iter()
        .map(|(k, v)| (k.to_string(), v.clone()))
        .collect()
}
fn table(rows: Vec<(&str, Cells)>) -> MemoryTableHandle {
    let t = MemoryStore::new().table("t");
    for (id, cells) in rows {
        t.upsert(id, record(&cells));
    }
    t
}
fn ids(t: &MemoryTable, q: &Query) -> Vec<String> {
    t.query(q).unwrap().into_iter().map(|(id, _)| id).collect()
}
fn found(t: &MemoryTable, c: MemoryCondition) -> Vec<String> {
    ids(t, &Query::new().filter(c))
}
fn cmp(path: &str, op: FilterOp, value: CborValue) -> MemoryCondition {
    MemoryCondition::cmp(path, op, value)
}

#[test]
fn ordered_ops_compare_numbers_across_int_and_float() {
    let t = table(vec![
        ("a", vec![("n", int(5))]),
        ("b", vec![("n", float(4.5))]),
        ("c", vec![("n", int(3))]),
    ]);
    assert_eq!(found(&t, cmp("n", FilterOp::Gt, int(4))), ["a", "b"]);
    assert_eq!(found(&t, cmp("n", FilterOp::Gte, float(5.0))), ["a"]);
    assert_eq!(found(&t, cmp("n", FilterOp::Lt, float(4.5))), ["c"]);
    assert_eq!(found(&t, cmp("n", FilterOp::Lte, int(5))), ["a", "b", "c"]);
    assert_eq!(found(&t, cmp("n", FilterOp::Eq, float(5.0))), ["a"]);
    assert_eq!(found(&t, cmp("n", FilterOp::Ne, int(5))), ["b", "c"]);
}

#[test]
fn values_of_different_kinds_never_match_ordered_ops() {
    let t = table(vec![
        ("text", vec![("v", text("5"))]),
        ("int", vec![("v", int(5))]),
        ("bool", vec![("v", CborValue::Bool(true))]),
    ]);
    assert_eq!(found(&t, cmp("v", FilterOp::Gt, int(1))), ["int"]);
    assert_eq!(found(&t, cmp("v", FilterOp::Lt, int(10))), ["int"]);
    assert_eq!(found(&t, cmp("v", FilterOp::Gte, text("0"))), ["text"]);
    assert_eq!(
        found(&t, cmp("v", FilterOp::Gt, CborValue::Bool(false))),
        ["bool"]
    );
}

#[test]
fn text_compares_by_code_point() {
    let t = table(vec![
        ("upper", vec![("v", text("B"))]),
        ("lower", vec![("v", text("a"))]),
    ]);
    assert_eq!(found(&t, cmp("v", FilterOp::Lt, text("a"))), ["upper"]);
    assert_eq!(found(&t, cmp("v", FilterOp::Gt, text("B"))), ["lower"]);
}

#[test]
fn a_null_cell_matches_only_what_the_null_rules_allow() {
    let t = table(vec![
        ("null", vec![("x", CborValue::Null)]),
        ("one", vec![("x", int(1))]),
        ("missing", vec![("y", int(1))]),
    ]);
    assert_eq!(
        found(&t, cmp("x", FilterOp::Eq, CborValue::Null)),
        ["null", "missing"]
    );
    assert_eq!(
        found(&t, cmp("x", FilterOp::Ne, int(1))),
        ["null", "missing"]
    );
    assert_eq!(found(&t, cmp("x", FilterOp::Gt, int(0))), ["one"]);
    assert_eq!(found(&t, cmp("x", FilterOp::Lt, int(5))), ["one"]);
    assert_eq!(
        found(&t, cmp("x", FilterOp::InSet, array(vec![int(1), int(2)]))),
        ["one"]
    );
    assert_eq!(
        found(
            &t,
            cmp("x", FilterOp::NotInSet, array(vec![int(1), int(2)]))
        ),
        ["null", "missing"]
    );
    assert_eq!(
        found(
            &t,
            cmp(
                "x",
                FilterOp::NotInSet,
                array(vec![CborValue::Null, int(1)])
            )
        ),
        Vec::<String>::new()
    );
}

#[test]
fn dotted_paths_read_nested_maps() {
    let address = |city: &str| CborValue::Map(vec![(text("city"), text(city))]);
    let t = table(vec![
        ("oslo", vec![("address", address("Oslo"))]),
        ("rome", vec![("address", address("Rome"))]),
        ("none", vec![("name", text("x"))]),
    ]);
    assert_eq!(
        found(&t, cmp("address.city", FilterOp::Eq, text("Oslo"))),
        ["oslo"]
    );
    assert_eq!(
        found(&t, cmp("address.city", FilterOp::Eq, CborValue::Null)),
        ["none"]
    );
    assert_eq!(
        found(&t, cmp("name.first", FilterOp::Eq, CborValue::Null)),
        ["oslo", "rome", "none"]
    );
    assert_eq!(
        found(&t, cmp("address.zip", FilterOp::Eq, CborValue::Null)),
        ["oslo", "rome", "none"]
    );
}

#[tokio::test]
async fn resolving_a_tree_without_deferred_parts_keeps_its_meaning() {
    let t = table(vec![
        ("a", vec![("n", int(1)), ("s", text("x"))]),
        ("b", vec![("n", int(2)), ("s", text("x"))]),
        ("c", vec![("n", int(3)), ("s", text("y"))]),
    ]);
    let tree = MemoryCondition::And(vec![
        MemoryCondition::Or(vec![
            cmp("n", FilterOp::Eq, int(1)),
            cmp("n", FilterOp::Eq, int(3)),
        ]),
        MemoryCondition::Not(Box::new(cmp("s", FilterOp::Eq, text("y")))),
    ]);
    let expected = found(&t, tree.clone());
    assert_eq!(expected, ["a"]);
    let resolved = tree.resolve().await.unwrap();
    assert_eq!(found(&t, resolved), expected);

    let q = Query::new()
        .filter(cmp("n", FilterOp::Gt, int(1)))
        .resolve()
        .await
        .unwrap();
    assert_eq!(ids(&t, &q), ["b", "c"]);
}

#[test]
fn a_column_named_with_a_dot_wins_over_the_path() {
    let nested = CborValue::Map(vec![(text("b"), int(2))]);
    let t = table(vec![("d", vec![("a.b", int(1)), ("a", nested)])]);
    assert_eq!(found(&t, cmp("a.b", FilterOp::Eq, int(1))), ["d"]);
    assert!(found(&t, cmp("a.b", FilterOp::Eq, int(2))).is_empty());
}

#[test]
fn like_is_case_insensitive_with_percent_and_underscore() {
    let t = table(vec![
        ("tea", vec![("name", text("Tea"))]),
        ("bag", vec![("name", text("tea-bag"))]),
        ("coffee", vec![("name", text("Coffee"))]),
        ("t", vec![("name", text("T"))]),
        ("num", vec![("name", int(42))]),
    ]);
    let like_ids = |pattern: &str| found(&t, cmp("name", FilterOp::Like, text(pattern)));
    assert_eq!(like_ids("t%"), ["tea", "bag", "t"]);
    assert_eq!(like_ids("_EA"), ["tea"]);
    assert_eq!(like_ids("%OFF%"), ["coffee"]);
    assert_eq!(like_ids("T"), ["t"]);
    assert_eq!(like_ids("4%"), ["num"]);
}

#[test]
fn and_or_not_combine_conditions() {
    let t = table(vec![
        ("a", vec![("n", int(1)), ("s", text("x"))]),
        ("b", vec![("n", int(2)), ("s", text("x"))]),
        ("c", vec![("n", int(3)), ("s", text("y"))]),
    ]);
    let n_gt_1 = || cmp("n", FilterOp::Gt, int(1));
    let s_x = || cmp("s", FilterOp::Eq, text("x"));
    assert_eq!(
        found(&t, MemoryCondition::And(vec![n_gt_1(), s_x()])),
        ["b"]
    );
    assert_eq!(
        found(&t, MemoryCondition::Or(vec![n_gt_1(), s_x()])),
        ["a", "b", "c"]
    );
    assert_eq!(found(&t, MemoryCondition::Not(Box::new(s_x()))), ["c"]);
    assert_eq!(found(&t, MemoryCondition::And(vec![])), ["a", "b", "c"]);
    assert!(found(&t, MemoryCondition::Or(vec![])).is_empty());
}

#[test]
fn several_filters_on_a_query_are_anded() {
    let t = table(vec![
        ("a", vec![("n", int(1))]),
        ("b", vec![("n", int(2))]),
        ("c", vec![("n", int(3))]),
    ]);
    let q = Query::new()
        .filter(cmp("n", FilterOp::Gt, int(1)))
        .filter(cmp("n", FilterOp::Lt, int(3)));
    assert_eq!(ids(&t, &q), ["b"]);
}

#[test]
fn a_bare_column_is_not_a_filter() {
    let t = table(vec![("a", vec![("n", int(1))])]);
    let rec = record(&[("n", int(1))]);
    assert!(MemoryCondition::Column("n".into()).matches(&rec).is_err());
    let q = Query::new().filter(MemoryCondition::Column("n".into()));
    assert!(t.query(&q).is_err());
}

fn catalog() -> MemoryTableHandle {
    table(vec![
        (
            "tea",
            vec![
                ("name", text("Tea")),
                ("qty", int(42)),
                ("hot", CborValue::Bool(true)),
            ],
        ),
        (
            "cake",
            vec![
                ("name", text("Cake")),
                ("qty", int(7)),
                ("hot", CborValue::Bool(false)),
            ],
        ),
        (
            "cocoa",
            vec![
                ("name", text("Cocoa")),
                ("note", text("TEA-like")),
                ("tags", array(vec![text("tea")])),
            ],
        ),
    ])
}

#[test]
fn search_reads_text_cells_and_number_cells_case_insensitively() {
    let t = catalog();
    assert_eq!(ids(&t, &Query::new().search("TEA")), ["tea", "cocoa"]);
    assert_eq!(ids(&t, &Query::new().search("42")), ["tea"]);
    assert_eq!(ids(&t, &Query::new().search("coc")), ["cocoa"]);
}

#[test]
fn search_does_not_read_booleans_or_arrays() {
    let t = catalog();
    assert!(ids(&t, &Query::new().search("true")).is_empty());
    assert!(ids(&t, &Query::new().search("false")).is_empty());
    let t = table(vec![("a", vec![("tags", array(vec![text("needle")]))])]);
    assert!(ids(&t, &Query::new().search("needle")).is_empty());
}

#[test]
fn search_reads_only_the_search_columns_when_given() {
    let t = catalog();
    let mut q = Query::new().search("tea");
    q.search_columns = vec!["name".into()];
    assert_eq!(ids(&t, &q), ["tea"]);
    q.search_columns = vec!["note".into()];
    assert_eq!(ids(&t, &q), ["cocoa"]);
    q.search_columns = vec!["qty".into()];
    assert!(ids(&t, &q).is_empty());
}

#[test]
fn search_combines_with_conditions() {
    let t = catalog();
    let q = Query::new()
        .filter(cmp("qty", FilterOp::Gt, int(10)))
        .search("tea");
    assert_eq!(ids(&t, &q), ["tea"]);
}

#[test]
fn a_search_condition_matches_like_a_search() {
    let t = catalog();
    assert_eq!(found(&t, MemoryCondition::search("CAKE")), ["cake"]);
    let scoped = MemoryCondition::Search {
        text: "tea".into(),
        columns: vec!["note".into()],
    };
    assert_eq!(found(&t, scoped), ["cocoa"]);
}

fn numbers(values: Vec<CborValue>) -> MemoryTableHandle {
    let t = MemoryStore::new().table("t");
    for (i, v) in values.into_iter().enumerate() {
        t.upsert(&format!("r{i}"), record(&[("n", v)]));
    }
    t
}

#[test]
fn ascending_puts_nulls_first_and_descending_reverses_the_whole_order() {
    let t = numbers(vec![int(2), CborValue::Null, int(1), int(1)]);
    let asc = Query::new().order_by("n", SortDirection::Ascending);
    assert_eq!(ids(&t, &asc), ["r1", "r2", "r3", "r0"]);
    let desc = Query::new().order_by("n", SortDirection::Descending);
    assert_eq!(ids(&t, &desc), ["r0", "r2", "r3", "r1"]);
}

#[test]
fn ordering_ties_keep_insertion_order() {
    let t = numbers(vec![int(1), int(1), int(1)]);
    for dir in [SortDirection::Ascending, SortDirection::Descending] {
        let q = Query::new().order_by("n", dir);
        assert_eq!(ids(&t, &q), ["r0", "r1", "r2"]);
    }
}

#[test]
fn ordering_compares_ints_and_floats_together() {
    let t = numbers(vec![int(3), float(1.5), int(2)]);
    let q = Query::new().order_by("n", SortDirection::Ascending);
    assert_eq!(ids(&t, &q), ["r1", "r2", "r0"]);
}

#[test]
fn ordering_a_mixed_column_is_total_and_keeps_each_kind_together() {
    let t = numbers(vec![text("b"), int(2), text("a"), int(1)]);
    let asc = ids(&t, &Query::new().order_by("n", SortDirection::Ascending));
    let mut desc = ids(&t, &Query::new().order_by("n", SortDirection::Descending));
    desc.reverse();
    assert_eq!(asc, desc, "descending is the reverse of ascending");
    let pos = |id: &str| asc.iter().position(|x| x == id).unwrap();
    assert_eq!(pos("r3") + 1, pos("r1"), "numbers are adjacent and ordered");
    assert_eq!(pos("r2") + 1, pos("r0"), "text is adjacent and ordered");
}

#[test]
fn nan_sorts_after_every_number() {
    let t = numbers(vec![float(f64::NAN), int(100), float(-1.0)]);
    let q = Query::new().order_by("n", SortDirection::Ascending);
    assert_eq!(ids(&t, &q), ["r2", "r1", "r0"]);
}

#[test]
fn later_order_keys_break_ties_of_earlier_ones() {
    let t = table(vec![
        ("a", vec![("cat", text("x")), ("price", int(1))]),
        ("b", vec![("cat", text("y")), ("price", int(5))]),
        ("c", vec![("cat", text("x")), ("price", int(9))]),
    ]);
    let q = Query::new()
        .order_by("cat", SortDirection::Ascending)
        .order_by("price", SortDirection::Descending);
    assert_eq!(ids(&t, &q), ["c", "a", "b"]);
}

#[test]
fn a_window_applies_after_filtering_and_ordering() {
    let t = numbers(vec![int(5), int(1), int(4), int(2), int(3), int(9)]);
    let base = Query::new()
        .filter(cmp("n", FilterOp::Lt, int(9)))
        .order_by("n", SortDirection::Ascending);
    assert_eq!(ids(&t, &base.clone().window(1, Some(2))), ["r3", "r4"]);
    assert_eq!(ids(&t, &base.clone().window(3, None)), ["r2", "r0"]);
    assert_eq!(ids(&t, &base.clone().window(0, Some(1))), ["r1"]);
}

#[test]
fn an_offset_past_the_end_is_empty() {
    let t = numbers(vec![int(1), int(2)]);
    assert!(ids(&t, &Query::new().window(2, None)).is_empty());
    assert!(ids(&t, &Query::new().window(10, Some(5))).is_empty());
}

#[test]
fn lookup_reads_columns_and_dotted_paths() {
    let rec = record(&[
        ("a", CborValue::Map(vec![(text("b"), int(2))])),
        ("a.b", int(1)),
        ("plain", int(7)),
    ]);
    assert_eq!(lookup(&rec, "plain"), Some(&int(7)));
    assert_eq!(lookup(&rec, "a.b"), Some(&int(1)));
    assert_eq!(lookup(&rec, "missing"), None);
    assert_eq!(lookup(&rec, "plain.deeper"), None);
}

#[test]
fn comparison_helpers_follow_the_documented_rules() {
    assert_eq!(cmp_values(&int(1), &float(1.5)), Some(Ordering::Less));
    assert_eq!(cmp_values(&float(2.0), &int(2)), Some(Ordering::Equal));
    assert_eq!(cmp_values(&text("a"), &int(1)), None);
    let datetime = CborValue::Tag(0, Box::new(text("2026-10-01T00:00:00Z")));
    assert_eq!(
        cmp_values(&datetime, &text("2026-10-05T00:00:00Z")),
        Some(Ordering::Less)
    );
    assert!(values_eq(&int(1), &float(1.0)));
    assert!(!values_eq(&int(1), &int(2)));
    assert!(values_eq(&datetime, &text("2026-10-01T00:00:00Z")));
    assert_eq!(text_of(&text("a")), Some("a".to_string()));
    assert_eq!(text_of(&int(42)), Some("42".to_string()));
    assert_eq!(text_of(&CborValue::Bool(true)), None);
    assert!(like("a%c", "ABBBC"));
    assert!(like("a_c", "abc"));
    assert!(!like("a_c", "abbc"));
}
