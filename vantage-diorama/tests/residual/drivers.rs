//! A paged chunk filter on a dotted path, over real drivers that refuse it:
//! the view fails loudly instead of presenting an unfiltered window as the
//! filtered set. (The REST shell refuses through the same
//! `condition_column_error`; it isn't a diorama dev-dependency.)

use vantage_core::Result;
use vantage_csv::Csv;
use vantage_diorama::{Clause, LoadState};
use vantage_sql::sqlite::SqliteDB;
use vantage_sql::sqlite::vista::SqliteVistaFactory;
use vantage_table::table::Table;
use vantage_types::EmptyEntity;
use vantage_vista::{Condition, FilterOp, Vista, VistaQuery};

use super::fixtures::open;
use crate::support::paged_dio;

/// `tests/fixtures/products.csv`: id, name, price.
fn csv_products() -> Result<Vista> {
    let csv = Csv::new(format!("{}/tests/fixtures", env!("CARGO_MANIFEST_DIR")));
    let table = Table::<Csv, EmptyEntity>::new("products", csv.clone())
        .with_id_column("id")
        .with_column_of::<String>("name")
        .with_column_of::<i64>("price");
    csv.vista_factory().from_table(table)
}

/// An in-memory sqlite `products` table with one row. The connection is
/// returned so it outlives the Vista's reads.
async fn sqlite_products() -> Result<(SqliteDB, Vista)> {
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .map_err(|e| vantage_core::error!("sqlite connect", detail = e.to_string()))?;
    let db = SqliteDB::new(pool);
    for statement in [
        "CREATE TABLE products (id TEXT PRIMARY KEY, name TEXT)",
        "INSERT INTO products (id, name) VALUES ('1', 'Espresso')",
    ] {
        sqlx::query(statement)
            .execute(db.pool())
            .await
            .map_err(|e| vantage_core::error!("sqlite setup", detail = e.to_string()))?;
    }
    let table = Table::<SqliteDB, EmptyEntity>::new("products", db.clone())
        .with_id_column("id")
        .with_column_of::<String>("name");
    let vista = SqliteVistaFactory::new(db.clone()).from_table::<EmptyEntity>(table)?;
    Ok((db, vista))
}

/// A paged view over `master` filtered on `name.first` fails naming the
/// path, and `Dio::fetch` hands the condition back.
async fn assert_dotted_filter_fails(master: Vista) -> Result<()> {
    let dio = paged_dio(master).await;
    let view = open(&dio, |b| {
        b.filter(Condition::new("name.first", FilterOp::Eq, "Espresso"))
    })
    .await;
    match view.load_state() {
        LoadState::Failed(failure) => {
            assert_eq!(failure.table, "products", "{failure}");
            assert_eq!(
                failure.clause,
                Some(Clause::Filter("name.first".into())),
                "{failure}"
            );
        }
        other => panic!("expected a loud failure, got {other:?}"),
    }
    assert_eq!(view.row_count(), 0);

    let condition = Condition::new("name.first", FilterOp::Eq, "Espresso");
    let query = VistaQuery::new().with_condition(condition.clone());
    let fetched = dio.fetch(0..10, &query).await?;
    assert_eq!(fetched.refused.conditions, vec![condition]);
    Ok(())
}

#[tokio::test]
async fn a_paged_dotted_filter_over_csv_fails_loudly() -> Result<()> {
    assert_dotted_filter_fails(csv_products()?).await
}

#[tokio::test]
async fn a_paged_dotted_filter_over_sqlite_fails_loudly() -> Result<()> {
    let (_db, vista) = sqlite_products().await?;
    assert_dotted_filter_fails(vista).await
}
