//! Typed writes: a YAML column's declared type decides the value written,
//! whatever loosely typed shape a form or a script hands over. A backend's
//! `vista` module supplies `async fn typed_events(suffix: &str) -> (DB,
//! String)`: an empty table with nullable columns `due` (date), `at`
//! (timestamp), `amount` (decimal, 2 places), `qty` (big integer), `starts`
//! (time) and `size` (bytes: an integer byte count), keyed by text `id`. `vista_tests!` invokes
//! it with the dialect's YAML driver key.

macro_rules! vista_typed_writes_tests {
    ($block:literal) => {
        mod typed_writes {
            use ciborium::Value as CborValue;
            use vantage_dataset::prelude::*;
            use vantage_types::Record;
            use vantage_vista::{Vista, VistaFactory};

            use super::{TestResult, typed_events};

            async fn events(suffix: &str) -> Vista {
                let (db, name) = typed_events(suffix).await;
                let block = $block;
                let yaml = format!(
                    r#"
name: {name}
columns:
  id: {{ type: string, flags: [id] }}
  due: {{ type: date }}
  at: {{ type: timestamp }}
  amount: {{ type: decimal }}
  qty: {{ type: int }}
  starts: {{ type: time }}
  size: {{ type: bytes }}
{block}:
  table: {name}
"#
                );
                db.vista_factory()
                    .from_yaml(&yaml)
                    .expect("typed vista loads")
            }

            fn row(cells: &[(&str, CborValue)]) -> Record<CborValue> {
                cells
                    .iter()
                    .map(|(k, v)| (k.to_string(), v.clone()))
                    .collect()
            }

            fn text(s: &str) -> CborValue {
                CborValue::Text(s.into())
            }

            /// The text a backend reads a date, time, instant or decimal back
            /// as: plain or inside its tag.
            fn inner(value: &CborValue) -> String {
                match value {
                    CborValue::Text(s) => s.clone(),
                    CborValue::Tag(_, inner) => inner_text(inner),
                    CborValue::Float(f) => f.to_string(),
                    CborValue::Integer(i) => i128::from(*i).to_string(),
                    other => panic!("not a scalar: {other:?}"),
                }
            }

            fn inner_text(value: &CborValue) -> String {
                match value {
                    CborValue::Text(s) => s.clone(),
                    other => panic!("tag without text: {other:?}"),
                }
            }

            fn instant(value: &CborValue) -> chrono::DateTime<chrono::Utc> {
                let s = inner(value);
                chrono::DateTime::parse_from_rfc3339(&s)
                    .or_else(|_| chrono::DateTime::parse_from_str(&s, "%Y-%m-%d %H:%M:%S%.f%#z"))
                    .map(|dt| dt.with_timezone(&chrono::Utc))
                    .or_else(|_| {
                        chrono::NaiveDateTime::parse_from_str(&s, "%Y-%m-%d %H:%M:%S%.f")
                            .map(|n| n.and_utc())
                    })
                    .unwrap_or_else(|_| panic!("not an instant: {s}"))
            }

            fn date(value: &CborValue) -> chrono::NaiveDate {
                chrono::NaiveDate::parse_from_str(&inner(value), "%Y-%m-%d")
                    .unwrap_or_else(|_| panic!("not a date: {value:?}"))
            }

            fn time(value: &CborValue) -> chrono::NaiveTime {
                chrono::NaiveTime::parse_from_str(&inner(value), "%H:%M:%S%.f")
                    .unwrap_or_else(|_| panic!("not a time: {value:?}"))
            }

            fn decimal(value: &CborValue) -> rust_decimal::Decimal {
                inner(value)
                    .parse::<rust_decimal::Decimal>()
                    .unwrap_or_else(|_| panic!("not a decimal: {value:?}"))
                    .normalize()
            }

            fn expected_instant() -> chrono::DateTime<chrono::Utc> {
                "2026-12-10T09:30:00Z".parse().unwrap()
            }

            /// Text from a form or a script is written as the column's type.
            #[tokio::test]
            async fn typed_write_types_text_by_column() -> TestResult {
                let vista = events("text").await;
                vista
                    .insert_value(
                        "t1",
                        &row(&[
                            ("due", text("2026-12-10")),
                            ("at", text("2026-12-10T11:30:00+02:00")),
                            ("amount", text("12.50")),
                            ("qty", text("3")),
                            ("starts", text("09:30:00")),
                            ("size", text("1536")),
                        ]),
                    )
                    .await?;
                let stored = vista.get_value("t1").await?.expect("inserted");
                assert_eq!(
                    date(&stored["due"]),
                    chrono::NaiveDate::from_ymd_opt(2026, 12, 10).unwrap()
                );
                assert_eq!(instant(&stored["at"]), expected_instant());
                assert_eq!(decimal(&stored["amount"]), "12.5".parse()?);
                assert_eq!(stored["qty"], CborValue::Integer(3.into()));
                assert_eq!(
                    time(&stored["starts"]),
                    chrono::NaiveTime::from_hms_opt(9, 30, 0).unwrap()
                );
                assert_eq!(stored["size"], CborValue::Integer(1536.into()));
                Ok(())
            }

            /// A form's datetime arrives SurrealDB-tagged (`Tag(12)`), a
            /// midnight instant on a date column is that date.
            #[tokio::test]
            async fn typed_write_takes_a_tagged_datetime() -> TestResult {
                let vista = events("tag12").await;
                let secs = expected_instant().timestamp();
                let tag12 = CborValue::Tag(
                    12,
                    Box::new(CborValue::Array(vec![
                        CborValue::Integer(secs.into()),
                        CborValue::Integer(0.into()),
                    ])),
                );
                let midnight = CborValue::Tag(0, Box::new(text("2026-12-10T00:00:00Z")));
                vista
                    .insert_value("t2", &row(&[("at", tag12), ("due", midnight)]))
                    .await?;
                let stored = vista.get_value("t2").await?.expect("inserted");
                assert_eq!(instant(&stored["at"]), expected_instant());
                assert_eq!(
                    date(&stored["due"]),
                    chrono::NaiveDate::from_ymd_opt(2026, 12, 10).unwrap()
                );
                Ok(())
            }

            /// A value that doesn't fit its column is refused as
            /// `InvalidValue` naming the column, never stored or nulled.
            #[tokio::test]
            async fn typed_write_refuses_what_doesnt_fit() -> TestResult {
                let vista = events("refuse").await;
                for (column, value) in [
                    ("due", text("next week")),
                    ("qty", text("many")),
                    ("at", text("soon")),
                ] {
                    let err = vista
                        .insert_value("t3", &row(&[(column, value)]))
                        .await
                        .expect_err("refused");
                    assert!(err.is_invalid_value(), "{column}: {err:?}");
                    assert!(err.to_string().contains(column), "{column}: {err}");
                }
                assert!(vista.get_value("t3").await?.is_none());
                Ok(())
            }

            /// `""` from a cleared form field and null both write no value.
            #[tokio::test]
            async fn typed_write_clears_with_empty_text() -> TestResult {
                let vista = events("clear").await;
                vista
                    .insert_value(
                        "t4",
                        &row(&[
                            ("due", text("")),
                            ("at", CborValue::Null),
                            ("qty", text("")),
                        ]),
                    )
                    .await?;
                let stored = vista.get_value("t4").await?.expect("inserted");
                assert_eq!(stored["due"], CborValue::Null);
                assert_eq!(stored["at"], CborValue::Null);
                assert_eq!(stored["qty"], CborValue::Null);
                Ok(())
            }

            /// A row read back and written unchanged stays unchanged.
            #[tokio::test]
            async fn typed_write_round_trip_keeps_the_row() -> TestResult {
                let vista = events("round").await;
                vista
                    .insert_value(
                        "t5",
                        &row(&[
                            ("due", text("2026-12-10")),
                            ("at", text("2026-12-10T09:30:00Z")),
                            ("amount", text("7.25")),
                            ("qty", CborValue::Integer(4.into())),
                            ("starts", text("18:05:00")),
                            ("size", CborValue::Integer(9.into())),
                        ]),
                    )
                    .await?;
                let first = vista.get_value("t5").await?.expect("inserted");
                let mut back = first.clone();
                back.shift_remove("id");
                vista.replace_value("t5", &back).await?;
                let second = vista.get_value("t5").await?.expect("replaced");
                assert_eq!(first, second);
                Ok(())
            }

            /// SQL has no duration type: the column is refused at load, not
            /// quietly stored as text.
            #[tokio::test]
            async fn typed_yaml_refuses_duration() -> TestResult {
                let (db, name) = typed_events("duration").await;
                let block = $block;
                let yaml = format!(
                    "name: {name}\ncolumns:\n  id: {{ type: string, flags: [id] }}\n  \
                     wait: {{ type: duration }}\n{block}:\n  table: {name}\n"
                );
                let err = db.vista_factory().from_yaml(&yaml).expect_err("refused");
                assert!(err.to_string().contains("not supported"), "{err}");
                Ok(())
            }
        }
    };
}
