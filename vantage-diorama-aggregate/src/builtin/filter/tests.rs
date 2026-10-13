use super::*;
use crate::builtin::{Count, Sum};

fn record(pairs: &[(&str, CborValue)]) -> Record<CborValue> {
    let mut r = Record::new();
    for (k, v) in pairs {
        r.insert((*k).to_string(), v.clone());
    }
    r
}

fn rows(records: Vec<Record<CborValue>>) -> Rows {
    records
        .into_iter()
        .enumerate()
        .map(|(i, r)| (i.to_string(), r))
        .collect()
}

fn text(s: &str) -> CborValue {
    CborValue::Text(s.to_string())
}

/// The ids of the rows `conditions` keep, in row order.
fn narrowed_ids(conditions: &Conditions, rows: &Rows) -> Vec<String> {
    conditions.narrow(rows).keys().cloned().collect()
}

fn events() -> Rows {
    rows(vec![
        record(&[
            ("Event", text("opened")),
            ("Size", CborValue::Integer(10.into())),
        ]),
        record(&[
            ("Event", text("failed")),
            ("Size", CborValue::Integer(20.into())),
        ]),
        record(&[
            ("Event", text("opened")),
            ("Size", CborValue::Integer(30.into())),
        ]),
    ])
}

#[test]
fn where_narrows_a_count() {
    let agg = Where::new(Conditions::new().eq("Event", text("opened")), Count::rows());
    assert_eq!(agg.compute(&events()), CborValue::Integer(2.into()));
}

/// The reason this is a combinator and not a `CountWhere` twin: the same
/// conditions narrow any reduction.
#[test]
fn where_narrows_a_sum() {
    let agg = Where::new(
        Conditions::new().eq("Event", text("opened")),
        Sum::new("Size"),
    );
    // 10 + 30, not 60.
    assert_eq!(agg.compute(&events()), CborValue::Integer(40.into()));
}

#[test]
fn no_conditions_passes_every_row_through() {
    let agg = Where::new(Conditions::new(), Count::rows());
    assert_eq!(agg.compute(&events()), CborValue::Integer(3.into()));
}

#[test]
fn a_term_on_an_absent_column_matches_nothing() {
    let conditions = Conditions::new().eq("Nope", text("opened"));
    assert!(narrowed_ids(&conditions, &events()).is_empty());
    let agg = Where::new(conditions, Count::rows());
    assert_eq!(agg.compute(&events()), CborValue::Integer(0.into()));
}

/// Configuration carries strings; the data may not. Matching only on CBOR
/// equality would make `code: 200` in a YAML file silently count zero.
#[test]
fn a_text_term_matches_a_numeric_column() {
    let numeric = rows(vec![
        record(&[("code", CborValue::Integer(200.into()))]),
        record(&[("code", CborValue::Integer(500.into()))]),
    ]);
    let agg = Where::new(Conditions::new().eq("code", text("200")), Count::rows());
    assert_eq!(agg.compute(&numeric), CborValue::Integer(1.into()));
}

/// A row whose `Message` is a nested map holding `Transport`.
fn nested_transport(transport: &str) -> Record<CborValue> {
    record(&[(
        "Message",
        CborValue::Map(vec![(text("Transport"), text(transport))]),
    )])
}

/// How many of `rows` have `Message.Transport == "smtp"`.
fn count_smtp(rows: &Rows) -> CborValue {
    Where::new(
        Conditions::new().eq("Message.Transport", text("smtp")),
        Count::rows(),
    )
    .compute(rows)
}

#[test]
fn terms_reach_into_nested_records_by_dotted_path() {
    let nested = rows(vec![nested_transport("smtp"), nested_transport("api")]);
    assert_eq!(count_smtp(&nested), CborValue::Integer(1.into()));
}

/// What a term means on ragged numeric data: an integer and a float of the
/// same number are equal both ways, a numeric text matches too, and a row
/// with the cell absent or null never matches a number.
#[test]
fn a_numeric_term_matches_int_float_and_text_but_not_absent_or_null() {
    let ragged = rows(vec![
        record(&[("Size", CborValue::Integer(3.into()))]),
        record(&[("Size", CborValue::Float(3.0))]),
        record(&[("Size", text("3"))]),
        record(&[("Size", CborValue::Float(3.5))]),
        record(&[("Size", CborValue::Null)]),
        record(&[("Other", CborValue::Integer(3.into()))]),
    ]);
    for expected in [CborValue::Integer(3.into()), CborValue::Float(3.0)] {
        let conditions = Conditions::new().eq("Size", expected.clone());
        assert_eq!(
            narrowed_ids(&conditions, &ragged),
            ["0", "1", "2"],
            "Size == {expected:?}"
        );
        let agg = Where::new(conditions, Count::rows());
        assert_eq!(
            agg.compute(&ragged),
            CborValue::Integer(3.into()),
            "Size == {expected:?}"
        );
    }
}

/// A flat key spelled with a dot (a joined column such as `author.name`) is
/// read as it is; only when no such key exists does the term descend into
/// nested maps.
#[test]
fn a_literal_dotted_key_is_read_before_the_nested_path() {
    let mixed = rows(vec![
        record(&[("Message.Transport", text("smtp"))]),
        nested_transport("smtp"),
        nested_transport("api"),
    ]);
    assert_eq!(count_smtp(&mixed), CborValue::Integer(2.into()));
}

#[test]
fn terms_are_anded() {
    let agg = Where::new(
        Conditions::new()
            .eq("Event", text("opened"))
            .eq("Size", CborValue::Integer(30.into())),
        Count::rows(),
    );
    assert_eq!(agg.compute(&events()), CborValue::Integer(1.into()));
}
