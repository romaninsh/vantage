//! `Conditions` and `Where`: which rows an aggregation sees, with each
//! operand converted to the type its column declares. And the numeric
//! reductions at their edges.

use std::borrow::Cow;

use ciborium::Value as CborValue;
use vantage_diorama::aggregate::{
    Aggregation, Avg, Conditions, Count, Distinct, GroupBy, Max, Min, Rows, Sum, Where,
};
use vantage_vista::{Column, VistaMetadata};

use crate::support::aggregate::{int, record, revenue, text};

fn rows(cells: &[(&str, CborValue)]) -> Rows {
    cells
        .iter()
        .map(|(id, value)| (id.to_string(), record(&[("c", value.clone())])))
        .collect()
}

/// A schema declaring column `c` with `original_type`.
fn typed(original_type: &str) -> VistaMetadata {
    VistaMetadata::new().with_column(Column::new("c", original_type))
}

/// How many of `rows` an `c == operand` term admits under `schema`.
fn matched(rows: &Rows, operand: CborValue, schema: &VistaMetadata) -> CborValue {
    Where::new(Conditions::new().eq("c", operand), Count::rows()).compute(rows, schema)
}

#[test]
fn a_text_operand_matches_an_integer_column() {
    let cells = rows(&[("a", int(200)), ("b", int(500))]);
    assert_eq!(matched(&cells, text("200"), &typed("i64")), int(1));
    assert_eq!(matched(&cells, text("200"), &typed("bigint")), int(1));
}

#[test]
fn a_fractional_text_operand_matches_an_integer_column_by_number() {
    let cells = rows(&[("a", int(3)), ("b", int(4))]);
    assert_eq!(matched(&cells, text("3.0"), &typed("i64")), int(1));
}

#[test]
fn a_text_operand_matches_a_float_column() {
    let cells = rows(&[("a", CborValue::Float(2.5)), ("b", CborValue::Float(3.0))]);
    assert_eq!(matched(&cells, text("2.5"), &typed("f64")), int(1));
}

#[test]
fn a_text_operand_matches_a_bool_column() {
    let cells = rows(&[("a", CborValue::Bool(true)), ("b", CborValue::Bool(false))]);
    assert_eq!(matched(&cells, text("true"), &typed("bool")), int(1));
    assert_eq!(matched(&cells, text("false"), &typed("bool")), int(1));
}

#[test]
fn a_scalar_operand_matches_a_text_column_as_its_text() {
    let cells = rows(&[
        ("a", text("7")),
        ("b", text("2.5")),
        ("c", text("true")),
        ("d", text("other")),
    ]);
    let schema = typed("varchar(8)");
    assert_eq!(matched(&cells, int(7), &schema), int(1));
    assert_eq!(matched(&cells, CborValue::Float(2.5), &schema), int(1));
    assert_eq!(matched(&cells, CborValue::Bool(true), &schema), int(1));
    assert_eq!(matched(&cells, text("other"), &schema), int(1));
}

/// Tags are transparent: a tagged scalar renders as what it wraps.
#[test]
fn a_tagged_operand_matches_a_text_column_as_what_it_wraps() {
    let cells = rows(&[("a", text("2026-10-01T00:00:00Z")), ("b", text("x"))]);
    let stamp = CborValue::Tag(0, Box::new(text("2026-10-01T00:00:00Z")));
    assert_eq!(matched(&cells, stamp, &typed("String")), int(1));
}

#[test]
fn a_non_scalar_operand_is_compared_as_given() {
    let cells = rows(&[("a", text("x"))]);
    let listed = CborValue::Array(vec![text("x")]);
    assert_eq!(matched(&cells, listed, &typed("String")), int(0));
}

#[test]
fn an_operand_that_doesnt_convert_is_compared_as_given() {
    let ints = rows(&[("a", int(1))]);
    assert_eq!(matched(&ints, text("n/a"), &typed("i64")), int(0));
    let bools = rows(&[("a", CborValue::Bool(true))]);
    assert_eq!(matched(&bools, text("yes"), &typed("bool")), int(0));
    let floats = rows(&[("a", CborValue::Float(1.0))]);
    assert_eq!(matched(&floats, text("one"), &typed("f64")), int(0));
}

#[test]
fn an_untyped_column_compares_the_operand_as_given() {
    let cells = rows(&[("a", int(200))]);
    let untyped = VistaMetadata::new().with_column(Column::untyped("c"));
    assert_eq!(matched(&cells, text("200"), &untyped), int(0));
    assert_eq!(matched(&cells, text("200"), &VistaMetadata::new()), int(0));
    assert_eq!(matched(&cells, int(200), &untyped), int(1));
}

/// Parsing `"0.1"` through a float would compare a different number.
#[test]
fn an_exact_decimal_operand_is_never_converted() {
    let cells = rows(&[("a", CborValue::Float(0.1))]);
    assert_eq!(
        matched(&cells, text("0.1"), &typed("numeric(10, 2)")),
        int(0)
    );
}

#[test]
fn a_term_on_an_absent_column_matches_nothing() {
    let cells = rows(&[("a", int(1)), ("b", int(2))]);
    let other = Where::new(Conditions::new().eq("missing", int(1)), Count::rows());
    assert_eq!(other.compute(&cells, &VistaMetadata::new()), int(0));
}

#[test]
fn a_null_cell_never_matches_a_number() {
    let cells = rows(&[("a", CborValue::Null), ("b", int(0))]);
    assert_eq!(matched(&cells, int(0), &typed("i64")), int(1));
}

#[test]
fn an_integer_and_a_float_of_one_number_are_equal() {
    let cells = rows(&[("a", CborValue::Float(3.0)), ("b", int(4))]);
    assert_eq!(matched(&cells, int(3), &VistaMetadata::new()), int(1));
}

#[test]
fn terms_are_anded() {
    let cells: Rows = [
        ("a", "open", "high"),
        ("b", "open", "low"),
        ("c", "done", "high"),
    ]
    .into_iter()
    .map(|(id, status, priority)| {
        let row = record(&[("status", text(status)), ("priority", text(priority))]);
        (id.to_string(), row)
    })
    .collect();
    let both = Conditions::new()
        .eq("status", "open")
        .eq("priority", "high");
    let narrowed = both.narrow(&cells, &VistaMetadata::new());
    assert_eq!(narrowed.keys().collect::<Vec<_>>(), ["a"]);
}

#[test]
fn a_dotted_term_reads_into_nested_records() {
    let nested = |country: &str| CborValue::Map(vec![(text("country"), text(country))]);
    let cells: Rows = [("a", "LV"), ("b", "EE")]
        .into_iter()
        .map(|(id, country)| (id.to_string(), record(&[("customer", nested(country))])))
        .collect();
    let latvians = Conditions::new().eq("customer.country", "LV");
    let narrowed = latvians.narrow(&cells, &VistaMetadata::new());
    assert_eq!(narrowed.keys().collect::<Vec<_>>(), ["a"]);
}

#[test]
fn a_declared_dotted_column_is_read_as_a_flat_key() {
    let cells: Rows = [("a", int(7)), ("b", int(8))]
        .into_iter()
        .map(|(id, value)| (id.to_string(), record(&[("a.b", value)])))
        .collect();
    let declared = VistaMetadata::new().with_column(Column::untyped("a.b"));
    let term = Conditions::new().eq("a.b", int(7));
    assert_eq!(term.narrow(&cells, &declared).len(), 1);
    assert_eq!(term.narrow(&cells, &VistaMetadata::new()).len(), 0);
}

#[test]
fn matches_judges_one_row_with_the_same_coercion() {
    let schema = typed("i64");
    let term = Conditions::new().eq("c", "200");
    assert!(term.matches(&record(&[("c", int(200))]), &schema));
    assert!(!term.matches(&record(&[("c", int(201))]), &schema));
}

#[test]
fn no_terms_pass_every_row_through_untouched() {
    let cells = rows(&[("a", int(1)), ("b", int(2))]);
    let none = Conditions::new();
    assert!(none.is_empty());
    let narrowed = none.narrow(&cells, &VistaMetadata::new());
    assert!(matches!(narrowed, Cow::Borrowed(_)));
    assert_eq!(narrowed.len(), 2);
}

/// A filtered `GroupBy` needs no special support from either side.
#[test]
fn a_where_wraps_a_group_by() {
    let cells: Rows = [("a", "open", 10), ("b", "done", 5), ("c", "open", 7)]
        .into_iter()
        .map(|(id, status, amount)| {
            let row = record(&[("status", text(status)), ("amount", int(amount))]);
            (id.to_string(), row)
        })
        .collect();
    let big = Where::new(
        Conditions::new().eq("amount", int(10)),
        GroupBy::column("status", revenue()),
    );
    let out = big.compute(&cells, &VistaMetadata::new());
    assert_eq!(out.len(), 1);
    assert_eq!(out.rows[0].1.get("revenue"), Some(&int(10)));
}

// ---- the numeric reductions at their edges ---------------------------------

#[test]
fn an_empty_input_sums_to_zero_and_has_no_mean_min_or_max() {
    let empty = Rows::new();
    let schema = VistaMetadata::new();
    assert_eq!(Sum::of("c").compute(&empty, &schema), int(0));
    assert_eq!(Avg::of("c").compute(&empty, &schema), CborValue::Null);
    assert_eq!(Min::of("c").compute(&empty, &schema), CborValue::Null);
    assert_eq!(Max::of("c").compute(&empty, &schema), CborValue::Null);
    assert_eq!(Distinct::of("c").compute(&empty, &schema), int(0));
}

#[test]
fn rows_without_a_number_are_skipped() {
    let cells = rows(&[("a", int(4)), ("b", text("x")), ("c", CborValue::Null)]);
    let schema = VistaMetadata::new();
    assert_eq!(Sum::of("c").compute(&cells, &schema), int(4));
    assert_eq!(Avg::of("c").compute(&cells, &schema), CborValue::Float(4.0));
    assert_eq!(Max::of("c").compute(&cells, &schema), int(4));
    assert_eq!(Min::of("c").compute(&cells, &schema), int(4));
    let words = rows(&[("a", text("x"))]);
    assert_eq!(Avg::of("c").compute(&words, &schema), CborValue::Null);
}

#[test]
fn the_mean_is_a_float() {
    let cells = rows(&[("a", int(1)), ("b", int(2))]);
    assert_eq!(
        Avg::of("c").compute(&cells, &VistaMetadata::new()),
        CborValue::Float(1.5)
    );
}

#[test]
fn min_and_max_span_ints_and_floats() {
    let cells = rows(&[("a", int(3)), ("b", CborValue::Float(1.5)), ("c", int(9))]);
    let schema = VistaMetadata::new();
    assert_eq!(Min::of("c").compute(&cells, &schema), CborValue::Float(1.5));
    assert_eq!(Max::of("c").compute(&cells, &schema), int(9));
}

#[test]
fn distinct_counts_each_value_once() {
    let cells = rows(&[
        ("a", text("x")),
        ("b", text("y")),
        ("c", text("x")),
        ("d", int(1)),
    ]);
    assert_eq!(
        Distinct::of("c").compute(&cells, &VistaMetadata::new()),
        int(3)
    );
}
