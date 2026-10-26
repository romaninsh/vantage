//! Transactional bulk import through the MySQL vista, and the helpers the
//! other `9_import_*` files share. Each test creates its own `imp_<test>`
//! table and drops it on the way out.

use std::error::Error;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_dataset::prelude::*;
use vantage_sql::mysql::{AnyMysqlType, MysqlDB};
use vantage_table::table::{AfterFn, BeforeFn, Hook, Phase, Table};
use vantage_types::{EmptyEntity, Record};
use vantage_vista::{Capability, FilterOp, Vista, VistaFactory};

pub(super) type TestResult = std::result::Result<(), Box<dyn Error>>;

pub(super) const MYSQL_URL: &str = "mysql://vantage:vantage@localhost:3306/vantage";

/// A fresh `imp_<test>` table, so tests running in parallel don't share rows.
pub(super) async fn setup(test: &str) -> (MysqlDB, String) {
    let db = MysqlDB::connect(MYSQL_URL).await.unwrap();
    let name = format!("imp_{test}");
    sqlx::query(&format!("DROP TABLE IF EXISTS `{name}`"))
        .execute(db.pool())
        .await
        .unwrap();
    sqlx::query(&format!(
        "CREATE TABLE `{name}` (id VARCHAR(64) PRIMARY KEY, title TEXT, status VARCHAR(32), sku VARCHAR(32))"
    ))
    .execute(db.pool())
    .await
    .unwrap();
    (db, name)
}

pub(super) async fn teardown(db: &MysqlDB, name: &str) {
    sqlx::query(&format!("DROP TABLE IF EXISTS `{name}`"))
        .execute(db.pool())
        .await
        .unwrap();
}

fn table(db: &MysqlDB, name: &str) -> Table<MysqlDB, EmptyEntity> {
    Table::new(name, db.clone())
        .with_id_column("id")
        .with_column_of::<String>("title")
        .with_column_of::<String>("status")
        .with_column_of::<String>("sku")
}

pub(super) fn vista(db: &MysqlDB, name: &str) -> Vista {
    db.vista_factory().from_table(table(db, name)).unwrap()
}

pub(super) fn open_vista(db: &MysqlDB, name: &str) -> Vista {
    let mut v = vista(db, name);
    v.add_condition("status", FilterOp::Eq, CborValue::Text("open".into()))
        .unwrap();
    v
}

pub(super) fn row(title: &str, status: &str) -> Record<CborValue> {
    let mut record = Record::new();
    record.insert("title".to_string(), CborValue::Text(title.into()));
    record.insert("status".to_string(), CborValue::Text(status.into()));
    record
}

pub(super) fn sku_row(sku: &str) -> Record<CborValue> {
    let mut record = row("t", "open");
    record.insert("sku".to_string(), CborValue::Text(sku.into()));
    record
}

pub(super) fn batch(rows: Vec<(&str, Record<CborValue>)>) -> IndexMap<String, Record<CborValue>> {
    rows.into_iter()
        .map(|(id, r)| (id.to_string(), r))
        .collect()
}

/// Every stored row as `(id, title, status, sku)`, ordered by id.
pub(super) async fn stored(
    db: &MysqlDB,
    name: &str,
) -> Vec<(String, Option<String>, Option<String>, Option<String>)> {
    sqlx::query_as(&format!(
        "SELECT id, title, status, sku FROM `{name}` ORDER BY id"
    ))
    .fetch_all(db.pool())
    .await
    .unwrap()
}

#[tokio::test]
async fn import_counts_only_new_ids() -> TestResult {
    let (db, name) = setup("counts").await;
    let v = vista(&db, &name);
    for id in ["a", "b"] {
        v.insert_value(id.to_string(), &row("old", "open")).await?;
    }
    let before = stored(&db, &name).await;

    let rows = batch(vec![
        ("a", row("changed", "done")),
        ("b", row("changed", "done")),
        ("c", row("fresh", "open")),
        ("d", row("fresh", "open")),
        ("e", row("fresh", "open")),
    ]);
    assert_eq!(v.import_values(&rows).await?, 3);

    let after = stored(&db, &name).await;
    assert_eq!(after.len(), 5);
    for existing in &before {
        assert!(
            after.contains(existing),
            "existing row changed: {existing:?}"
        );
    }
    for (id, title, _, _) in &after[2..] {
        assert_eq!(title.as_deref(), Some("fresh"), "row {id}");
    }

    teardown(&db, &name).await;
    Ok(())
}

#[tokio::test]
async fn import_runs_insert_hooks_for_new_records_only() -> TestResult {
    let (db, name) = setup("hooks").await;
    let after_calls = Arc::new(AtomicUsize::new(0));
    let calls = after_calls.clone();
    let before: BeforeFn<MysqlDB> = Arc::new(|cx| {
        Box::pin(async move {
            cx.record.insert(
                "status".to_string(),
                AnyMysqlType::from("defaulted".to_string()),
            );
            Ok(())
        })
    });
    let after: AfterFn<MysqlDB> = Arc::new(move |_cx| {
        let calls = calls.clone();
        Box::pin(async move {
            calls.fetch_add(1, Ordering::SeqCst);
            Ok(())
        })
    });
    let hooked = table(&db, &name)
        .with_hook(Hook::BeforeInsert(Phase::Populate, before))
        .with_hook(Hook::AfterInsert(after));
    let v = db.vista_factory().from_table(hooked)?;

    sqlx::query(&format!(
        "INSERT INTO `{name}` (id, title, status) VALUES ('old', 'old', 'kept')"
    ))
    .execute(db.pool())
    .await?;
    let rows = batch(vec![
        ("old", row("changed", "x")),
        ("n1", row("fresh", "x")),
        ("n2", row("fresh", "x")),
    ]);
    assert_eq!(v.import_values(&rows).await?, 2);

    let after = stored(&db, &name).await;
    let status = |id: &str| {
        after
            .iter()
            .find(|r| r.0 == id)
            .and_then(|r| r.2.clone())
            .unwrap()
    };
    assert_eq!(status("n1"), "defaulted");
    assert_eq!(status("n2"), "defaulted");
    assert_eq!(status("old"), "kept");
    assert_eq!(after_calls.load(Ordering::SeqCst), 2);

    teardown(&db, &name).await;
    Ok(())
}

#[tokio::test]
async fn import_empty_is_zero() -> TestResult {
    let (db, name) = setup("empty").await;
    assert_eq!(vista(&db, &name).import_values(&IndexMap::new()).await?, 0);
    teardown(&db, &name).await;
    Ok(())
}

#[tokio::test]
async fn import_capability_on_writable_vistas_only() -> TestResult {
    use vantage_sql::mysql::vista::MysqlVistaSpec;
    use vantage_vista::SpecResolver;

    let (db, name) = setup("caps").await;

    let base_yaml = format!(
        r#"
name: base
columns:
  id: {{ type: string, flags: [id] }}
  title: {{ type: string }}
mysql:
  table: {name}
"#
    );
    let derived_yaml = r#"
name: derived
columns: {}
mysql:
  base: base
  inherit:
    columns: [id, title]
"#;
    let base_spec: MysqlVistaSpec = serde_yaml_ng::from_str(&base_yaml)?;
    let derived_spec: MysqlVistaSpec = serde_yaml_ng::from_str(derived_yaml)?;
    let map: IndexMap<String, MysqlVistaSpec> = [("base".to_string(), base_spec.clone())]
        .into_iter()
        .collect();
    let map = Arc::new(map);
    let resolver: Arc<dyn SpecResolver<MysqlVistaSpec>> =
        Arc::new(move |n: &str| map.get(n).cloned());
    let factory = db.vista_factory().with_resolver(resolver);

    assert!(
        factory
            .build_from_spec(base_spec)?
            .capabilities()
            .has(Capability::Import)
    );
    let derived = factory.build_from_spec(derived_spec)?;
    assert!(!derived.capabilities().has(Capability::Import));
    let rows = batch(vec![("a", row("a", "open"))]);
    assert!(derived.import_values(&rows).await.is_err());
    assert!(stored(&db, &name).await.is_empty());

    teardown(&db, &name).await;
    Ok(())
}
