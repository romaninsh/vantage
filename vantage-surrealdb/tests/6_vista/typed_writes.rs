//! Writes typed by the declared column (lender #1, #3, #13): date text into a
//! `datetime` field, `table:key` text into a `record<…>` field, no value as
//! `NONE`, and the schema mismatch report.

use std::sync::{Arc, mpsc};
use std::time::Duration;

use chrono::{DateTime, Utc};
use ciborium::Value as CborValue;
use vantage_dataset::contract::check_round_trip;
use vantage_dataset::prelude::*;
use vantage_expressions::ExprDataSource;
use vantage_surrealdb::surreal_expr;
use vantage_surrealdb::surrealdb::SurrealDB;
use vantage_surrealdb::vista::SurrealVistaSpec;
use vantage_table::table::{BuildKind, RollupOp, Table, TableInjector};
use vantage_types::{EmptyEntity, Record};
use vantage_vista::{SpecResolver, VistaFactory};

use super::{TestResult, setup};

fn record(fields: Vec<(&str, CborValue)>) -> Record<CborValue> {
    fields
        .into_iter()
        .map(|(k, v)| (k.to_string(), v))
        .collect()
}

fn text(s: &str) -> CborValue {
    CborValue::Text(s.into())
}

fn tag12(seconds: i64) -> CborValue {
    CborValue::Tag(
        12,
        Box::new(CborValue::Array(vec![
            CborValue::Integer(seconds.into()),
            CborValue::Integer(0.into()),
        ])),
    )
}

fn link(table: &str, key: &str) -> CborValue {
    CborValue::Tag(8, Box::new(CborValue::Array(vec![text(table), text(key)])))
}

/// Run each `;`-separated statement of `sql`.
async fn define(db: &SurrealDB, sql: &str) -> TestResult {
    for statement in sql.split(';').map(str::trim).filter(|s| !s.is_empty()) {
        db.execute(&surreal_expr!(statement)).await?;
    }
    Ok(())
}

// 2026-12-10T00:00:00Z and 2026-12-10T10:00:00Z.
const DEC_10: i64 = 1_796_860_800;
const DEC_10_AT_10: i64 = DEC_10 + 36_000;

/// lender #1: `type: date` / `type: timestamp` text reaches a `TYPE datetime`
/// field as a datetime; a date column refuses a time of day, and text that
/// isn't a date is `InvalidValue` naming the column.
#[tokio::test]
async fn date_and_timestamp_text_is_written_as_a_datetime() -> TestResult {
    let (db, _) = setup().await;
    define(
        &db,
        "DEFINE TABLE loan SCHEMAFULL; \
         DEFINE FIELD due_date ON loan TYPE datetime; \
         DEFINE FIELD funded_at ON loan TYPE datetime;",
    )
    .await?;
    let vista = db.vista_factory().from_yaml(
        r#"
name: loan
columns:
  id: { type: thing, flags: [id] }
  due_date: { type: date }
  funded_at: { type: timestamp }
"#,
    )?;

    vista
        .insert_value(
            "a",
            &record(vec![
                ("due_date", text("2026-12-10")),
                ("funded_at", text("2026-12-10T10:00:00Z")),
            ]),
        )
        .await?;
    vista
        .insert_value(
            "b",
            &record(vec![
                ("due_date", text("2026-12-10T00:00:00Z")),
                ("funded_at", text("2026-12-10")),
            ]),
        )
        .await?;
    let a = vista.get_value("a").await?.expect("inserted");
    let b = vista.get_value("b").await?.expect("inserted");
    assert_eq!(a.get("due_date"), Some(&tag12(DEC_10)));
    assert_eq!(a.get("funded_at"), Some(&tag12(DEC_10_AT_10)));
    assert_eq!(b.get("due_date"), Some(&tag12(DEC_10)));
    assert_eq!(b.get("funded_at"), Some(&tag12(DEC_10)));

    let later = record(vec![("due_date", text("2026-12-10T10:00:00Z"))]);
    let err = vista
        .patch_value("a", &later)
        .await
        .expect_err("not a date");
    assert!(err.is_invalid_value(), "{err:?}");
    let err = vista
        .patch_value("a", &record(vec![("funded_at", text("next week"))]))
        .await
        .expect_err("not a timestamp");
    assert!(err.is_invalid_value(), "{err:?}");
    assert!(format!("{err:?}").contains("funded_at"), "{err:?}");

    // A datetime another writer stored at 10:00 is the store's own value:
    // the row read and written back unchanged stays unchanged.
    db.execute(&surreal_expr!(
        "CREATE loan:c SET due_date = d'2026-12-10T10:00:00Z', funded_at = d'2026-12-10T10:00:00Z'"
    ))
    .await?;
    let c = vista.get_value("c").await?.expect("created");
    assert_eq!(c.get("due_date"), Some(&tag12(DEC_10_AT_10)));
    vista.replace_value("c", &c).await?;
    assert_eq!(vista.get_value("c").await?, Some(c));
    Ok(())
}

/// lender #3: on a `references:` column, `table:key` text and a bare key are
/// written as a record link; a link to another table is refused; a plain
/// text column keeps `table:key` as text.
#[tokio::test]
async fn reference_text_is_written_as_a_record_link() -> TestResult {
    let (db, _) = setup().await;
    define(
        &db,
        "DEFINE TABLE session SCHEMAFULL; \
         DEFINE FIELD login ON session TYPE record<login>; \
         DEFINE FIELD label ON session TYPE string;",
    )
    .await?;
    let vista = db.vista_factory().from_yaml(
        r#"
name: session
columns:
  id: { type: thing, flags: [id] }
  login: { type: string, references: login }
  label: { type: string }
"#,
    )?;

    vista
        .insert_value(
            "a",
            &record(vec![
                ("login", text("login:bob")),
                ("label", text("login:bob")),
            ]),
        )
        .await?;
    vista
        .insert_value(
            "b",
            &record(vec![("login", text("ann")), ("label", text("x"))]),
        )
        .await?;
    let a = vista.get_value("a").await?.expect("inserted");
    let b = vista.get_value("b").await?.expect("inserted");
    assert_eq!(a.get("login"), Some(&link("login", "bob")));
    assert_eq!(a.get("label"), Some(&text("login:bob")));
    assert_eq!(b.get("login"), Some(&link("login", "ann")));

    let err = vista
        .patch_value("a", &record(vec![("login", text("client:bob"))]))
        .await
        .expect_err("a link to another table");
    assert!(err.is_invalid_value(), "{err:?}");
    Ok(())
}

/// lender #13: no value (`()` from Rhai is null, a cleared form field is
/// `""`) on a typed column is written as `NONE`, which an `option<…>` field
/// takes and a patch uses to remove the field.
#[tokio::test]
async fn no_value_on_a_typed_column_is_written_as_none() -> TestResult {
    let (db, _) = setup().await;
    define(
        &db,
        "DEFINE TABLE loan SCHEMAFULL; \
         DEFINE FIELD closed_at ON loan TYPE option<datetime>; \
         DEFINE FIELD amount ON loan TYPE option<decimal>; \
         DEFINE FIELD borrower ON loan TYPE option<record<client>>;",
    )
    .await?;
    let vista = db.vista_factory().from_yaml(
        r#"
name: loan
columns:
  id: { type: thing, flags: [id] }
  closed_at: { type: timestamp }
  amount: { type: decimal }
  borrower: { type: string, references: client }
"#,
    )?;

    vista
        .insert_value(
            "a",
            &record(vec![
                ("closed_at", CborValue::Null),
                ("amount", text("")),
                ("borrower", CborValue::Null),
            ]),
        )
        .await?;
    vista
        .patch_value(
            "a",
            &record(vec![
                ("closed_at", text("2026-12-10")),
                ("amount", CborValue::Float(10.5)),
                ("borrower", text("client:ann")),
            ]),
        )
        .await?;
    let set = vista.get_value("a").await?.expect("patched");
    assert_eq!(set.get("closed_at"), Some(&tag12(DEC_10)));
    assert_eq!(
        set.get("amount"),
        Some(&CborValue::Tag(10, Box::new(text("10.5"))))
    );

    vista
        .patch_value(
            "a",
            &record(vec![
                ("closed_at", CborValue::Null),
                ("amount", CborValue::Null),
                ("borrower", text("")),
            ]),
        )
        .await?;
    let cleared = vista.get_value("a").await?.expect("patched");
    for column in ["closed_at", "amount", "borrower"] {
        assert!(
            matches!(
                cleared.get(column),
                None | Some(CborValue::Null) | Some(CborValue::Tag(6, _))
            ),
            "{column}: {:?}",
            cleared.get(column)
        );
    }

    // A Rust-typed table declaring `Option<DateTime<Utc>>` types a value as
    // `DateTime<Utc>` does.
    let table = Table::<SurrealDB, EmptyEntity>::new("loan", db.clone())
        .with_id_column("id")
        .with_column_of::<Option<DateTime<Utc>>>("closed_at");
    db.vista_factory()
        .from_table(table)?
        .patch_value("a", &record(vec![("closed_at", text("2026-12-10"))]))
        .await?;
    let set = vista.get_value("a").await?.expect("patched");
    assert_eq!(set.get("closed_at"), Some(&tag12(DEC_10)));
    Ok(())
}

/// Every type in the YAML vocabulary loads, and a row read and written back
/// unchanged stays unchanged.
#[tokio::test]
async fn a_row_of_every_declared_type_round_trips() -> TestResult {
    let (db, _) = setup().await;
    define(
        &db,
        "CREATE kinds:a SET n = 4, f = 1.5, b = true, s = 'x', d = d'2026-12-10T00:00:00Z', \
         ts = d'2026-12-10T10:00:00.5Z', t = '10:30:00', dur = 1h30m, size = 2048, \
         amount = 10.50dec, who = login:bob, rec = login:ann;",
    )
    .await?;
    let vista = db.vista_factory().from_yaml(
        r#"
name: kinds
columns:
  id: { type: thing, flags: [id] }
  n: { type: int }
  f: { type: float }
  b: { type: bool }
  s: { type: string }
  d: { type: date }
  ts: { type: datetime }
  t: { type: time }
  dur: { type: duration }
  size: { type: bytes }
  amount: { type: decimal }
  who: { type: string, references: login }
  rec: { type: record }
"#,
    )?;
    check_round_trip(&vista, "kinds:a".to_string()).await;

    vista
        .patch_value(
            "a",
            &record(vec![("t", text("09:15")), ("dur", text("2h"))]),
        )
        .await?;
    let patched = vista.get_value("a").await?.expect("patched");
    assert_eq!(patched.get("t"), Some(&text("09:15")));
    assert!(
        matches!(patched.get("dur"), Some(CborValue::Tag(14, _))),
        "{:?}",
        patched.get("dur")
    );
    Ok(())
}

/// `check_schema` reads each `DEFINE FIELD` type; a YAML column whose type
/// disagrees comes back as a `SchemaMismatch` naming both, and a field that
/// can't hold `NONE` is reported for a typed column. Nothing is reported
/// before the check ran.
#[tokio::test]
async fn schema_mismatches_name_the_disagreeing_columns() -> TestResult {
    let (db, _) = setup().await;
    define(
        &db,
        "DEFINE TABLE loan SCHEMAFULL; \
         DEFINE FIELD due_date ON loan TYPE datetime; \
         DEFINE FIELD funded_at ON loan TYPE option<datetime>; \
         DEFINE FIELD closed_at ON loan TYPE null | datetime; \
         DEFINE FIELD borrower ON loan TYPE record<client>; \
         DEFINE FIELD lender ON loan TYPE option<record<client>>; \
         DEFINE FIELD amount ON loan TYPE decimal; \
         DEFINE FIELD note ON loan TYPE string;",
    )
    .await?;
    let spec = serde_yaml_ng::from_str(
        r#"
name: loan
columns:
  id: { type: thing, flags: [id] }
  due_date: { type: string }
  funded_at: { type: timestamp }
  closed_at: { type: timestamp }
  borrower: { type: string }
  lender: { type: string, references: client }
  amount: { type: decimal }
  note: { type: int }
  extra: { type: string }
"#,
    )?;
    let factory = db.vista_factory();
    let table = factory.table_from_spec(&spec)?;
    assert!(db.schema_mismatches(&table).is_empty(), "not checked yet");

    db.check_schema().await?;
    let mut found: Vec<(String, String, String)> = db
        .schema_mismatches(&table)
        .into_iter()
        .map(|m| {
            assert_eq!(m.table, "loan");
            assert!(!m.hint.is_empty(), "{m:?}");
            (m.column, m.declared, m.stored)
        })
        .collect();
    found.sort();
    let expected = [
        ("borrower", "string", "record<client>"),
        ("closed_at", "timestamp", "null | datetime"),
        ("due_date", "string", "datetime"),
        ("note", "int", "string"),
    ];
    let expected: Vec<(String, String, String)> = expected
        .iter()
        .map(|(c, d, s)| (c.to_string(), d.to_string(), s.to_string()))
        .collect();
    assert_eq!(found, expected);
    let _ = factory.build_from_spec(spec)?;
    Ok(())
}

/// Reporting mismatches builds each has-one target, and that build may read
/// the schema check again (a roll-up injected on the target asks whether its
/// link is a reference). The report must not hold the schema while it does.
#[tokio::test]
async fn schema_mismatches_build_a_has_one_target_that_reads_the_schema() -> TestResult {
    let (db, _) = setup().await;
    define(
        &db,
        "DEFINE TABLE client SCHEMAFULL; \
         DEFINE FIELD name ON client TYPE string; \
         DEFINE TABLE loan SCHEMAFULL; \
         DEFINE FIELD client ON loan TYPE record<client>; \
         DEFINE FIELD amount ON loan TYPE decimal;",
    )
    .await?;
    db.check_schema().await?;
    let loan: SurrealVistaSpec = serde_yaml_ng::from_str(
        r#"
name: loan
columns:
  id: { type: thing, flags: [id] }
  client: { type: string, references: client }
  amount: { type: decimal }
"#,
    )?;
    let client: SurrealVistaSpec = serde_yaml_ng::from_str(
        r#"
name: client
columns:
  id: { type: thing, flags: [id] }
  name: { type: string }
references:
  loans: { table: loan, kind: has_many, foreign_key: client }
"#,
    )?;
    let specs = [("loan", loan.clone()), ("client", client)];
    let resolver: Arc<dyn SpecResolver<SurrealVistaSpec>> = Arc::new(move |name: &str| {
        specs
            .iter()
            .find(|(key, _)| *key == name)
            .map(|(_, spec)| spec.clone())
    });
    let injector: TableInjector<SurrealDB> = Arc::new(|cx, table| {
        if cx.key == "client" && matches!(cx.kind, BuildKind::Reference { .. }) {
            *table = table
                .clone()
                .with_rollup("loans", "loans", RollupOp::Count, None, None)?;
        }
        Ok(())
    });
    let factory = db
        .vista_factory()
        .with_resolver(resolver)
        .with_injector(injector);

    let (done, finished) = mpsc::channel();
    std::thread::spawn(move || {
        let table = factory.table_from_spec(&loan).map(|table| {
            let mismatches = db.schema_mismatches(&table);
            (factory.build_from_spec(loan).is_ok(), mismatches)
        });
        let _ = done.send(table.map_err(|e| e.to_string()));
    });
    let (built, mismatches) = finished
        .recv_timeout(Duration::from_secs(3))
        .expect("schema_mismatches deadlocked on the schema lock")?;
    assert!(built);
    assert!(mismatches.is_empty(), "{mismatches:?}");
    Ok(())
}
