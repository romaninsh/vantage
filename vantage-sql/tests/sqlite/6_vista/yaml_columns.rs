//! YAML column declarations: flags reaching the vista column, `lazy:`
//! columns the vista computes per record, and `date` / `time` / `timestamp`
//! columns read back as typed values from the text SQLite keeps.

use std::sync::Arc;

use ciborium::Value as CborValue;
use vantage_dataset::prelude::*;
use vantage_sql::sqlite::SqliteDB;
use vantage_sql::sqlite::vista::SqliteVistaSpec;
use vantage_types::Record;
use vantage_vista::{ColumnFlag, FilterOp, SortDirection, SpecResolver, Vista, VistaFactory};

use super::{TestResult, setup, typed_events};

const EVENTS_YAML: &str = r#"
name: events
columns:
  id: { type: string, flags: [id] }
  due: { type: date }
  at: { type: timestamp }
  starts: { type: time }
sqlite:
  table: events
"#;

async fn events() -> (SqliteDB, Vista) {
    let (db, _) = typed_events("").await;
    let vista = db.vista_factory().from_yaml(EVENTS_YAML).expect("loads");
    (db, vista)
}

fn tagged(tag: u64, text: &str) -> CborValue {
    CborValue::Tag(tag, Box::new(CborValue::Text(text.into())))
}

fn cells(cells: &[(&str, &str)]) -> Record<CborValue> {
    cells
        .iter()
        .map(|(k, v)| (k.to_string(), CborValue::Text(v.to_string())))
        .collect()
}

/// `(due, at, starts)` exactly as SQLite holds them.
async fn stored_text(db: &SqliteDB, id: &str) -> (String, String, String) {
    sqlx::query_as("SELECT due || '', at || '', starts || '' FROM events WHERE id = ?")
        .bind(id)
        .fetch_one(db.pool())
        .await
        .unwrap()
}

async fn seed(db: &SqliteDB, id: &str, due: &str, at: &str, starts: &str) {
    sqlx::query("INSERT INTO events (id, due, at, starts) VALUES (?, ?, ?, ?)")
        .bind(id)
        .bind(due)
        .bind(at)
        .bind(starts)
        .execute(db.pool())
        .await
        .unwrap();
}

/// A written date, time and timestamp read back tagged, as Postgres and
/// MySQL return them, and `HH:MM` is a valid time of day on both columns.
#[tokio::test]
async fn vista_temporal_columns_read_typed() -> TestResult {
    let (db, vista) = events().await;
    for (id, at, starts) in [
        ("full", "2026-12-10T09:30:00Z", "09:30:00"),
        ("short", "2026-12-10T09:30", "09:30"),
        ("naive", "2026-12-10 09:30:00", "09:30:00.250"),
    ] {
        let written = vista
            .insert_value(
                id,
                &cells(&[("due", "2026-12-10"), ("at", at), ("starts", starts)]),
            )
            .await?;
        assert_eq!(written["due"], tagged(1004, "2026-12-10"), "{id}");
        assert_eq!(
            written["at"],
            tagged(0, "2026-12-10T09:30:00.000000Z"),
            "{id}"
        );
    }

    let rows = vista.list_values().await?;
    assert_eq!(rows["full"]["starts"], tagged(101, "09:30:00.000000"));
    assert_eq!(rows["short"]["starts"], tagged(101, "09:30:00.000000"));
    assert_eq!(rows["naive"]["starts"], tagged(101, "09:30:00.250000"));
    assert_eq!(
        rows["short"]["at"],
        tagged(0, "2026-12-10T09:30:00.000000Z")
    );

    // Naive text is stored as the UTC instant it stands for, in the carrier's text.
    for id in ["full", "short", "naive"] {
        let (due, at, _) = stored_text(&db, id).await;
        assert_eq!(due, "2026-12-10", "{id}");
        assert_eq!(at, "2026-12-10T09:30:00.000000Z", "{id}");
    }
    assert_eq!(stored_text(&db, "short").await.2, "09:30:00.000000");
    Ok(())
}

/// Text another writer left in the database reads as the declared type, and
/// the read leaves the stored text alone.
#[tokio::test]
async fn vista_temporal_columns_read_legacy_text() -> TestResult {
    let (db, vista) = events().await;
    let legacy = [
        (
            "space",
            "2026-12-10 09:30:00",
            "09:30",
            "2026-12-10T09:30:00.000000Z",
            "09:30:00.000000",
        ),
        (
            "space_frac",
            "2026-12-10 09:30:00.250",
            "09:30:00",
            "2026-12-10T09:30:00.250000Z",
            "09:30:00.000000",
        ),
        (
            "zulu",
            "2026-12-10T09:30:00Z",
            "09:30:00.250",
            "2026-12-10T09:30:00.000000Z",
            "09:30:00.250000",
        ),
        (
            "zulu_frac",
            "2026-12-10T09:30:00.250Z",
            "23:59:59",
            "2026-12-10T09:30:00.250000Z",
            "23:59:59.000000",
        ),
        (
            "t_naive",
            "2026-12-10T09:30:00",
            "00:00",
            "2026-12-10T09:30:00.000000Z",
            "00:00:00.000000",
        ),
        (
            "minutes",
            "2026-12-10 09:30",
            "09:30",
            "2026-12-10T09:30:00.000000Z",
            "09:30:00.000000",
        ),
        (
            "offset",
            "2026-12-10T11:30:00+02:00",
            "09:30",
            "2026-12-10T09:30:00.000000Z",
            "09:30:00.000000",
        ),
        (
            "day",
            "2026-12-10",
            "09:30",
            "2026-12-10T00:00:00.000000Z",
            "09:30:00.000000",
        ),
    ];
    for (id, at, starts, _, _) in legacy {
        seed(&db, id, "2026-12-10", at, starts).await;
    }

    let rows = vista.list_values().await?;
    for (id, at, starts, typed_at, typed_starts) in legacy {
        assert_eq!(rows[id]["due"], tagged(1004, "2026-12-10"), "{id}");
        assert_eq!(rows[id]["at"], tagged(0, typed_at), "{id}");
        assert_eq!(rows[id]["starts"], tagged(101, typed_starts), "{id}");
        let one = vista.get_value(id).await?.expect("row");
        assert_eq!(one["at"], tagged(0, typed_at), "{id}");

        let (_, stored_at, stored_starts) = stored_text(&db, id).await;
        assert_eq!((stored_at.as_str(), stored_starts.as_str()), (at, starts));
    }

    // No value stays no value.
    sqlx::query("INSERT INTO events (id) VALUES ('empty')")
        .execute(db.pool())
        .await?;
    let empty = vista.get_value("empty").await?.expect("row");
    assert_eq!(empty["due"], CborValue::Null);
    assert_eq!(empty["at"], CborValue::Null);
    assert_eq!(empty["starts"], CborValue::Null);
    Ok(())
}

/// Stored text that is not the declared type fails the read, naming the
/// column and the value.
#[tokio::test]
async fn vista_temporal_column_refuses_unreadable_text() -> TestResult {
    for (column, due, at, starts, bad) in [
        (
            "due",
            "next week",
            "2026-12-10T09:30:00Z",
            "09:30:00",
            "next week",
        ),
        ("at", "2026-12-10", "soon", "09:30:00", "soon"),
        (
            "starts",
            "2026-12-10",
            "2026-12-10T09:30:00Z",
            "25:99",
            "25:99",
        ),
    ] {
        let (db, vista) = events().await;
        seed(&db, "bad", due, at, starts).await;
        for err in [
            vista.list_values().await.expect_err("list refused"),
            vista.get_value("bad").await.expect_err("get refused"),
        ] {
            let text = format!("{err:?}");
            assert!(text.contains(column) && text.contains(bad), "{text}");
        }
    }
    Ok(())
}

/// A condition and an order on a timestamp column compare the stored instant.
#[tokio::test]
async fn vista_timestamp_column_filters_and_orders() -> TestResult {
    let (_db, vista) = events().await;
    for (id, at) in [
        ("b", "2026-12-10T12:00:00Z"),
        ("a", "2026-12-10T09:30:00Z"),
        ("c", "2026-12-11T08:00:00Z"),
    ] {
        vista
            .insert_value(id, &cells(&[("at", at), ("starts", "09:30")]))
            .await?;
    }

    let mut later = vista.clone();
    later.add_condition("at", FilterOp::Gt, tagged(0, "2026-12-10T09:30:00Z"))?;
    later.set_order(Some(("at", SortDirection::Descending)))?;
    let ids: Vec<_> = later.list_values().await?.into_keys().collect();
    assert_eq!(ids, ["c", "b"]);

    let mut exact = vista.clone();
    exact.add_condition(
        "at",
        FilterOp::Eq,
        CborValue::Text("2026-12-10T12:00:00Z".into()),
    )?;
    let rows = exact.list_values().await?;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows["b"]["at"], tagged(0, "2026-12-10T12:00:00.000000Z"));
    Ok(())
}

/// SQLite keeps a `json` column as JSON text, keys in the order written, and
/// stored text that isn't a JSON object or array fails the read.
#[tokio::test]
async fn vista_json_column_stores_json_text() -> TestResult {
    let db = setup().await;
    sqlx::query("CREATE TABLE docs (id TEXT PRIMARY KEY, meta TEXT)")
        .execute(db.pool())
        .await?;
    let yaml = "name: docs\ncolumns:\n  id: { type: string, flags: [id] }\n  \
                meta: { type: json }\nsqlite:\n  table: docs\n";
    let vista = db.vista_factory().from_yaml(yaml)?;
    let meta = CborValue::Map(vec![
        ("tags".into(), CborValue::Array(vec!["a".into()])),
        ("n".into(), 1.into()),
    ]);
    let record: Record<CborValue> = [("meta".to_string(), meta)].into_iter().collect();
    vista.insert_value("d1", &record).await?;
    let (text,): (String,) = sqlx::query_as("SELECT meta FROM docs WHERE id = 'd1'")
        .fetch_one(db.pool())
        .await?;
    assert_eq!(text, r#"{"tags":["a"],"n":1}"#);

    sqlx::query("INSERT INTO docs VALUES ('bad', 'not json')")
        .execute(db.pool())
        .await?;
    let err = vista
        .get_value("bad")
        .await
        .expect_err("unreadable text refused");
    assert!(format!("{err:?}").contains("not json"), "{err:?}");
    Ok(())
}

/// The YAML flags with a typed counterpart reach the vista column through
/// the typed table. An id flagged `auto` makes the vista id auto, so scripted
/// inserts may leave the id to the database.
#[tokio::test]
async fn vista_yaml_column_flags_reach_the_vista() -> TestResult {
    let db = setup().await;
    let yaml = r#"
name: product
columns:
  id: { type: string, flags: [id] }
  name: { type: string, flags: [searchable, mandatory] }
  is_deleted: { type: bool, flags: [label] }
"#;
    let vista = db.vista_factory().from_yaml(yaml)?;

    let name = vista.column("name").expect("name");
    assert!(name.has_flag(ColumnFlag::Searchable), "{name:?}");
    assert!(name.has_flag(ColumnFlag::Mandatory), "{name:?}");
    let is_deleted = vista.column("is_deleted").expect("is_deleted");
    assert!(
        is_deleted.has_flag(ColumnFlag::from("label")),
        "{is_deleted:?}"
    );
    let id = vista.column("id").expect("id");
    assert!(!id.has_flag(ColumnFlag::Auto), "{id:?}");

    let auto_yaml = "name: product\ncolumns:\n  id: { type: int, flags: [id, auto] }\n  name: { type: string }\n";
    let auto = db.vista_factory().from_yaml(auto_yaml)?;
    let id = auto.column("id").expect("id");
    assert!(id.has_flag(ColumnFlag::Auto), "{id:?}");

    // A vista derived from it keeps the auto id, unless it picks another id
    // column.
    let base: SqliteVistaSpec = serde_yaml_ng::from_str(auto_yaml)?;
    let resolver: Arc<dyn SpecResolver<SqliteVistaSpec>> =
        Arc::new(move |name: &str| (name == "product").then(|| base.clone()));
    let derived = |columns: &str, extra: &str| {
        let yaml = format!(
            "name: derived\ncolumns: {columns}\n{extra}sqlite:\n  base: product\n  inherit:\n    columns: [id, name]\n"
        );
        db.vista_factory()
            .with_resolver(resolver.clone())
            .build_from_spec(serde_yaml_ng::from_str(&yaml).unwrap())
            .unwrap()
    };
    let id = derived("{}", "").column("id").cloned().expect("id");
    assert!(id.has_flag(ColumnFlag::Auto), "{id:?}");
    let name = derived("{}", "id_column: name\n")
        .column("name")
        .cloned()
        .expect("name");
    assert!(!name.has_flag(ColumnFlag::Auto), "{name:?}");
    // ...and takes Auto from the new id column's own flag.
    let code = derived(
        "{ code: { type: int, flags: [auto] } }",
        "id_column: code\n",
    )
    .column("code")
    .cloned()
    .expect("code");
    assert!(code.has_flag(ColumnFlag::Auto), "{code:?}");
    Ok(())
}

/// A `lazy:` column runs its Rhai script on each returned record (`row` in
/// scope), in declaration order — the second script reads the first's value.
#[cfg(feature = "rhai")]
#[tokio::test]
async fn vista_yaml_lazy_columns_compute_from_row() -> TestResult {
    let db = setup().await;

    let yaml = r#"
name: product_view
columns:
  id:
    type: string
    flags: [id]
  name:
    type: string
  price:
    type: int
  price_label:
    lazy: '`${row.name}: ${row.price}`'
  shouted:
    lazy: 'row.price_label.to_upper()'
sqlite:
  table: product
"#;

    let vista = db.vista_factory().from_yaml(yaml)?;
    let rows = vista.list_values().await?;
    assert_eq!(
        rows["a"].get("price_label"),
        Some(&CborValue::Text("Alpha: 10".into()))
    );
    assert_eq!(
        rows["a"].get("shouted"),
        Some(&CborValue::Text("ALPHA: 10".into()))
    );

    // Computed by the Vista, so every read path fills it and writes drop it.
    assert!(vista.column("shouted").unwrap().is_computed());
    let window = vista.fetch_window(2, 1).await?;
    assert_eq!(
        window.rows[0].1.get("price_label"),
        Some(&CborValue::Text("Gamma: 30".into()))
    );
    let record: Record<CborValue> = [
        ("name", CborValue::Text("Delta".into())),
        ("price", CborValue::Integer(40i64.into())),
        ("price_label", CborValue::Text("stale".into())),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v))
    .collect();
    vista.insert_value("d", &record).await?;
    let fetched = vista.get_value("d").await?.expect("inserted");
    assert_eq!(
        fetched.get("price_label"),
        Some(&CborValue::Text("Delta: 40".into()))
    );
    Ok(())
}
