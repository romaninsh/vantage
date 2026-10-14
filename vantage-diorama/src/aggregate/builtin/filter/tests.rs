use vantage_vista::Column;

use super::*;
use crate::aggregate::builtin::fixtures::{events, record, rows, schema, text};
use crate::aggregate::builtin::{Count, Sum};

/// The ids of the rows `conditions` keep, in row order.
fn narrowed_ids(conditions: &Conditions, rows: &Rows, schema: &VistaMetadata) -> Vec<String> {
    conditions.narrow(rows, schema).keys().cloned().collect()
}

#[test]
fn where_narrows_a_count() {
    let agg = Where::new(Conditions::new().eq("Event", text("opened")), Count::rows());
    assert_eq!(
        agg.compute(&events(), &schema()),
        CborValue::Integer(2.into())
    );
}

/// The reason this is a combinator and not a counting twin: the same
/// conditions narrow any reduction.
#[test]
fn where_narrows_a_sum() {
    let agg = Where::new(
        Conditions::new().eq("Event", text("opened")),
        Sum::of("Size"),
    );
    // 10 + 30, not 60.
    assert_eq!(
        agg.compute(&events(), &schema()),
        CborValue::Integer(40.into())
    );
}

/// An augmented Dio hydrates what a `Where` judges as well as what it reduces.
#[test]
fn where_reads_its_terms_and_the_inner_columns() {
    let agg = Where::new(
        Conditions::new()
            .eq("Event", text("opened"))
            .eq("Size", CborValue::Integer(10.into())),
        Sum::of("Size"),
    );
    assert_eq!(
        agg.reads(),
        Reads::Columns(vec!["Event".to_string(), "Size".to_string()])
    );
}

#[test]
fn no_conditions_passes_every_row_through() {
    let agg = Where::new(Conditions::new(), Count::rows());
    assert_eq!(
        agg.compute(&events(), &schema()),
        CborValue::Integer(3.into())
    );
}

#[test]
fn a_term_on_an_absent_column_matches_nothing() {
    let conditions = Conditions::new().eq("Nope", text("opened"));
    assert!(narrowed_ids(&conditions, &events(), &schema()).is_empty());
    let agg = Where::new(conditions, Count::rows());
    assert_eq!(
        agg.compute(&events(), &schema()),
        CborValue::Integer(0.into())
    );
}

fn int(i: i64) -> CborValue {
    CborValue::Integer(i.into())
}

/// How many of `rows` have `code == operand`, judged against `schema`.
fn count_code(rows: &Rows, operand: CborValue, schema: &VistaMetadata) -> CborValue {
    Where::new(Conditions::new().eq("code", operand), Count::rows()).compute(rows, schema)
}

/// Configuration carries strings; the data may not. A YAML `code: "200"`
/// over a column the schema types as an integer counts the integer rows.
#[test]
fn a_text_term_matches_a_typed_integer_column() {
    let numeric = rows(vec![
        record(&[("code", int(200))]),
        record(&[("code", int(500))]),
    ]);
    let typed = VistaMetadata::new().with_column(Column::new("code", "i64"));
    assert_eq!(count_code(&numeric, text("200"), &typed), int(1));
}

#[test]
fn a_numeric_term_matches_a_typed_text_column() {
    let textual = rows(vec![
        record(&[("code", text("200"))]),
        record(&[("code", text("500"))]),
    ]);
    let typed = VistaMetadata::new().with_column(Column::new("code", "String"));
    assert_eq!(count_code(&textual, int(200), &typed), int(1));
}

/// Nothing declares the type, so nothing says which side to convert: the
/// operand compares as given, and `"200"` is not `200`.
#[test]
fn an_untyped_column_matches_the_operand_as_given() {
    let numeric = rows(vec![
        record(&[("code", int(200))]),
        record(&[("code", int(500))]),
    ]);
    let declared = VistaMetadata::new().with_column(Column::untyped("code"));
    for schema in [schema(), declared] {
        assert_eq!(count_code(&numeric, text("200"), &schema), int(0));
        assert_eq!(count_code(&numeric, int(200), &schema), int(1));
    }
}

/// Ragged data under a float column: an integer, a float and a coerced text
/// operand all match the integer and float cells of the same number. A null
/// or absent cell never matches, and neither does a text cell, because only
/// the operand is converted.
#[test]
fn a_typed_float_term_matches_int_and_float_cells_but_not_null() {
    let ragged = rows(vec![
        record(&[("Size", int(3))]),
        record(&[("Size", CborValue::Float(3.0))]),
        record(&[("Size", CborValue::Float(3.5))]),
        record(&[("Size", CborValue::Null)]),
        record(&[("Other", int(3))]),
        record(&[("Size", text("3"))]),
    ]);
    let typed = VistaMetadata::new().with_column(Column::new("Size", "f64"));
    for operand in [int(3), CborValue::Float(3.0), text("3")] {
        let conditions = Conditions::new().eq("Size", operand.clone());
        assert_eq!(
            narrowed_ids(&conditions, &ragged, &typed),
            ["0", "1"],
            "Size == {operand:?}"
        );
    }
}

/// A row whose `Message` is a nested map holding `Transport`.
fn nested_transport(transport: &str) -> Record<CborValue> {
    record(&[(
        "Message",
        CborValue::Map(vec![(text("Transport"), text(transport))]),
    )])
}

/// How many of `rows` have `Message.Transport == "smtp"`, judged against
/// `schema`.
fn count_smtp(rows: &Rows, schema: &VistaMetadata) -> CborValue {
    Where::new(
        Conditions::new().eq("Message.Transport", text("smtp")),
        Count::rows(),
    )
    .compute(rows, schema)
}

/// One row holding the literal `Message.Transport` key and one nested row,
/// both `smtp`.
fn flat_and_nested() -> Rows {
    rows(vec![
        record(&[("Message.Transport", text("smtp"))]),
        nested_transport("smtp"),
    ])
}

#[test]
fn terms_reach_into_nested_records_by_dotted_path() {
    let nested = rows(vec![nested_transport("smtp"), nested_transport("api")]);
    assert_eq!(count_smtp(&nested, &schema()), int(1));
}

/// A joined column such as SQL's `author.name` is declared whole, so the
/// term reads the flat key and never descends.
#[test]
fn a_declared_dotted_column_is_read_flat() {
    let declared = VistaMetadata::new().with_column(Column::untyped("Message.Transport"));
    let conditions = Conditions::new().eq("Message.Transport", text("smtp"));
    assert_eq!(
        narrowed_ids(&conditions, &flat_and_nested(), &declared),
        ["0"]
    );
    assert_eq!(count_smtp(&flat_and_nested(), &declared), int(1));
}

#[test]
fn an_undeclared_dotted_column_is_read_as_a_path() {
    let conditions = Conditions::new().eq("Message.Transport", text("smtp"));
    assert_eq!(
        narrowed_ids(&conditions, &flat_and_nested(), &schema()),
        ["1"]
    );
    assert_eq!(count_smtp(&flat_and_nested(), &schema()), int(1));
}

#[test]
fn terms_are_anded() {
    let agg = Where::new(
        Conditions::new()
            .eq("Event", text("opened"))
            .eq("Size", CborValue::Integer(30.into())),
        Count::rows(),
    );
    assert_eq!(
        agg.compute(&events(), &schema()),
        CborValue::Integer(1.into())
    );
}
