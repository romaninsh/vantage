//! `Table<Csv, _>` reads: typing by column, ids, conditions, and the
//! read-only contract.

use vantage_core::Result;
use vantage_csv::operation::CsvOperation;
use vantage_csv::{AnyCsvType, Csv, CsvTypeVariants};
use vantage_dataset::prelude::{InsertableValueSet, ReadableValueSet, WritableValueSet};
use vantage_expressions::traits::expressive::ExpressiveEnum;
use vantage_expressions::{Expression, Expressive};
use vantage_table::column::core::Column;
use vantage_table::table::Table;
use vantage_table::traits::table_source::TableSource;
use vantage_types::{EmptyEntity, Record};

fn csv() -> Csv {
    Csv::new(format!("{}/data", env!("CARGO_MANIFEST_DIR")))
}

#[tokio::test]
async fn declared_column_types_drive_parsing() -> Result<()> {
    let table = Table::<Csv, EmptyEntity>::new("bakery", csv())
        .with_column_of::<String>("name")
        .with_column_of::<i64>("profit_margin");

    let rows = table.list_values().await?;
    assert_eq!(rows.len(), 1);

    let bakery = &rows["hill_valley"];
    assert_eq!(
        bakery["name"].try_get::<String>().as_deref(),
        Some("Hill Valley Bakery")
    );
    assert_eq!(bakery["profit_margin"].try_get::<i64>(), Some(15));
    assert_eq!(
        bakery["profit_margin"].type_variant(),
        Some(CsvTypeVariants::Int)
    );
    Ok(())
}

#[tokio::test]
async fn bool_and_json_columns_parse_to_their_variants() -> Result<()> {
    let table = Table::<Csv, EmptyEntity>::new("client", csv())
        .with_column_of::<String>("name")
        .with_column_of::<bool>("is_paying_client")
        .with_column_of::<serde_json::Value>("metadata");

    let rows = table.list_values().await?;
    assert_eq!(rows.len(), 3);

    assert_eq!(
        rows["marty"]["is_paying_client"].try_get::<bool>(),
        Some(true)
    );
    assert_eq!(
        rows["biff"]["is_paying_client"].try_get::<bool>(),
        Some(false)
    );

    let metadata = &rows["biff"]["metadata"];
    assert_eq!(metadata.type_variant(), Some(CsvTypeVariants::Json));
    let json = metadata.try_get::<serde_json::Value>().expect("json");
    assert_eq!(json["credit_limit"], serde_json::json!(500));
    Ok(())
}

#[tokio::test]
async fn embedded_json_inventory_parses_when_column_is_json() -> Result<()> {
    let table = Table::<Csv, EmptyEntity>::new("product", csv())
        .with_column_of::<i64>("calories")
        .with_column_of::<serde_json::Value>("inventory");

    let rows = table.list_values().await?;
    assert_eq!(rows.len(), 5);
    let cupcake = &rows["flux_cupcake"];
    assert_eq!(cupcake["calories"].try_get::<i64>(), Some(300));
    let inventory = cupcake["inventory"]
        .try_get::<serde_json::Value>()
        .expect("json");
    assert_eq!(inventory["stock"], serde_json::json!(50));
    Ok(())
}

#[tokio::test]
async fn columns_without_a_declared_type_stay_strings() -> Result<()> {
    let table = Table::<Csv, EmptyEntity>::new("product", csv());

    let rows = table.list_values().await?;
    let cupcake = &rows["flux_cupcake"];
    assert_eq!(
        cupcake["calories"].type_variant(),
        Some(CsvTypeVariants::String)
    );
    assert_eq!(
        cupcake["calories"].try_get::<String>().as_deref(),
        Some("300")
    );
    Ok(())
}

#[tokio::test]
async fn empty_field_reads_as_untyped_null() -> Result<()> {
    let table = Table::<Csv, EmptyEntity>::new("client", csv())
        .with_column_of::<serde_json::Value>("metadata");

    let rows = table.list_values().await?;
    assert_eq!(rows["marty"]["metadata"].type_variant(), None);
    assert_eq!(rows["marty"]["metadata"].value(), "");
    Ok(())
}

#[tokio::test]
async fn aliased_column_reads_the_aliased_header_under_the_column_name() -> Result<()> {
    let table = Table::<Csv, EmptyEntity>::new("bakery", csv())
        .with_column(Column::<String>::new("title").with_alias("name"))
        .with_column(Column::<i64>::new("margin").with_alias("profit_margin"));

    let rows = table.list_values().await?;
    let bakery = &rows["hill_valley"];
    assert_eq!(
        bakery["title"].try_get::<String>().as_deref(),
        Some("Hill Valley Bakery")
    );
    assert_eq!(bakery["margin"].try_get::<i64>(), Some(15));
    assert!(bakery.get("name").is_none());
    assert!(bakery.get("profit_margin").is_none());
    Ok(())
}

#[tokio::test]
async fn table_without_columns_surfaces_every_header() -> Result<()> {
    let table = Table::<Csv, EmptyEntity>::new("client", csv());

    let rows = table.list_values().await?;
    let marty = &rows["marty"];
    for header in [
        "id",
        "name",
        "email",
        "contact_details",
        "is_paying_client",
        "metadata",
    ] {
        assert!(marty.get(header).is_some(), "missing header {header}");
    }
    assert_eq!(marty["email"].value(), "marty@gmail.com");
    Ok(())
}

#[tokio::test]
async fn id_column_defaults_to_id() -> Result<()> {
    let table = Table::<Csv, EmptyEntity>::new("client", csv());

    let ids: Vec<String> = table.list_values().await?.into_keys().collect();
    assert_eq!(ids, ["marty", "doc", "biff"]);
    Ok(())
}

#[tokio::test]
async fn with_id_column_picks_the_record_identifier() -> Result<()> {
    let table = Table::<Csv, EmptyEntity>::new("client", csv().with_id_column("name"));

    let rows = table.list_values().await?;
    assert_eq!(rows.len(), 3);
    assert!(rows.contains_key("Doc Brown"));
    assert!(!rows.contains_key("doc"));
    Ok(())
}

#[tokio::test]
async fn missing_id_column_falls_back_to_row_indices_from_zero() -> Result<()> {
    let table = Table::<Csv, EmptyEntity>::new("client", csv().with_id_column("no_such_column"));

    let ids: Vec<String> = table.list_values().await?.into_keys().collect();
    assert_eq!(ids, ["0", "1", "2"]);
    Ok(())
}

#[tokio::test]
async fn get_value_returns_the_record_for_an_id() -> Result<()> {
    let table = Table::<Csv, EmptyEntity>::new("client", csv())
        .with_column_of::<String>("name")
        .with_column_of::<String>("email");

    let doc = table.get_value("doc").await?.expect("doc exists");
    assert_eq!(
        doc["name"].try_get::<String>().as_deref(),
        Some("Doc Brown")
    );
    assert_eq!(
        doc["email"].try_get::<String>().as_deref(),
        Some("doc@brown.com")
    );
    Ok(())
}

#[tokio::test]
async fn get_value_of_unknown_id_is_none() -> Result<()> {
    let table = Table::<Csv, EmptyEntity>::new("client", csv());

    assert!(table.get_value("nonexistent").await?.is_none());
    Ok(())
}

#[tokio::test]
async fn get_some_value_returns_a_row_of_the_table() -> Result<()> {
    let table = Table::<Csv, EmptyEntity>::new("bakery", csv()).with_column_of::<String>("name");

    let (id, record) = table.get_some_value().await?.expect("a row");
    assert_eq!(id, "hill_valley");
    assert_eq!(
        record["name"].try_get::<String>().as_deref(),
        Some("Hill Valley Bakery")
    );
    Ok(())
}

#[tokio::test]
async fn count_is_the_number_of_data_rows() -> Result<()> {
    let table = Table::<Csv, EmptyEntity>::new("product", csv());

    let count = table.data_source().get_table_count(&table).await?;
    assert_eq!(count, 5);
    Ok(())
}

#[tokio::test]
async fn missing_file_is_an_error() {
    let table = Table::<Csv, EmptyEntity>::new("nonexistent", csv());

    assert!(table.list_values().await.is_err());
    assert!(table.get_value("x").await.is_err());
}

#[tokio::test]
async fn every_write_operation_fails() {
    let table = Table::<Csv, EmptyEntity>::new("bakery", csv());
    let record = Record::new();

    assert!(table.insert_value("x", &record).await.is_err());
    assert!(table.replace_value("hill_valley", &record).await.is_err());
    assert!(table.patch_value("hill_valley", &record).await.is_err());
    assert!(table.delete("hill_valley").await.is_err());
    assert!(table.delete_all().await.is_err());
    assert!(table.insert_return_id_value(&record).await.is_err());
}

#[tokio::test]
async fn search_is_unsupported_and_never_matches_everything() {
    let mut table =
        Table::<Csv, EmptyEntity>::new("client", csv()).with_column_of::<String>("name");
    table.add_search("marty");

    let err = table
        .list_values()
        .await
        .expect_err("CSV has no full-table search");
    assert!(err.is_unsupported(), "expected Unsupported, got {err:?}");
}

#[tokio::test]
async fn eq_condition_on_bool_column_filters_rows() -> Result<()> {
    let mut table = Table::<Csv, EmptyEntity>::new("client", csv())
        .with_column_of::<String>("name")
        .with_column_of::<bool>("is_paying_client");
    table.add_condition(table["is_paying_client"].eq(AnyCsvType::new(true)));

    let rows = table.list_values().await?;
    assert_eq!(rows.keys().collect::<Vec<_>>(), ["marty", "doc"]);
    Ok(())
}

#[tokio::test]
async fn eq_condition_on_string_column_filters_rows() -> Result<()> {
    let mut table =
        Table::<Csv, EmptyEntity>::new("client", csv()).with_column_of::<String>("name");
    table.add_condition(table["name"].eq(AnyCsvType::new("Doc Brown".to_string())));

    let rows = table.list_values().await?;
    assert_eq!(rows.keys().collect::<Vec<_>>(), ["doc"]);
    Ok(())
}

#[tokio::test]
async fn eq_condition_on_int_column_filters_rows() -> Result<()> {
    let mut table =
        Table::<Csv, EmptyEntity>::new("product", csv()).with_column_of::<i64>("calories");
    table.add_condition(table["calories"].eq(AnyCsvType::new(300_i64)));

    let rows = table.list_values().await?;
    assert_eq!(rows.keys().collect::<Vec<_>>(), ["flux_cupcake"]);
    Ok(())
}

#[tokio::test]
async fn eq_condition_accepts_a_native_value() -> Result<()> {
    let mut table = Table::<Csv, EmptyEntity>::new("product", csv())
        .with_column_of::<String>("name")
        .with_column_of::<i64>("price");
    table.add_condition(table["price"].eq(120_i64));
    table.add_condition(table["name"].eq("Flux Capacitor Cupcake"));

    let rows = table.list_values().await?;
    assert_eq!(rows.keys().collect::<Vec<_>>(), ["flux_cupcake"]);
    Ok(())
}

#[tokio::test]
async fn several_conditions_all_apply() -> Result<()> {
    let mut table = Table::<Csv, EmptyEntity>::new("product", csv())
        .with_column_of::<i64>("calories")
        .with_column_of::<bool>("is_deleted");
    table.add_condition(table["is_deleted"].eq(AnyCsvType::new(false)));
    table.add_condition(table["calories"].eq(AnyCsvType::new(300_i64)));

    let rows = table.list_values().await?;
    assert_eq!(rows.keys().collect::<Vec<_>>(), ["flux_cupcake"]);

    table.add_condition(table["calories"].eq(AnyCsvType::new(999_i64)));
    assert!(table.list_values().await?.is_empty());
    Ok(())
}

#[tokio::test]
async fn in_condition_matches_values_of_another_table_column() -> Result<()> {
    let csv = csv();
    let mut paying = Table::<Csv, EmptyEntity>::new("client", csv.clone())
        .with_column_of::<String>("name")
        .with_column_of::<bool>("is_paying_client");
    paying.add_condition(paying["is_paying_client"].eq(AnyCsvType::new(true)));

    let name_column = csv.create_column::<String>("name");
    let paying_names = csv.column_table_values_expr(&paying, &name_column);

    let mut clients =
        Table::<Csv, EmptyEntity>::new("client", csv.clone()).with_column_of::<String>("name");
    clients.add_condition(clients["name"].in_(paying_names.expr()));

    let rows = clients.list_values().await?;
    assert_eq!(rows.keys().collect::<Vec<_>>(), ["marty", "doc"]);
    Ok(())
}

#[tokio::test]
async fn column_table_values_expr_lists_the_column_of_the_filtered_table() -> Result<()> {
    let csv = csv();
    let mut table = Table::<Csv, EmptyEntity>::new("client", csv.clone())
        .with_column_of::<String>("name")
        .with_column_of::<bool>("is_paying_client");
    let name_column = csv.create_column::<String>("name");

    let all = csv.column_table_values_expr(&table, &name_column);
    let all = csv_execute(&csv, all.expression()).await?;
    assert_eq!(all, ["Marty McFly", "Doc Brown", "Biff Tannen"]);

    table.add_condition(table["is_paying_client"].eq(AnyCsvType::new(true)));
    let paying = csv.column_table_values_expr(&table, &name_column);
    let paying = csv_execute(&csv, paying.expression()).await?;
    assert_eq!(paying, ["Marty McFly", "Doc Brown"]);
    Ok(())
}

async fn csv_execute(csv: &Csv, expr: &Expression<AnyCsvType>) -> Result<Vec<String>> {
    use vantage_expressions::traits::datasource::ExprDataSource;

    let list = csv.execute(expr).await?;
    let items = list.try_get::<Vec<AnyCsvType>>().expect("a list value");
    Ok(items.iter().map(|v| v.value().clone()).collect())
}

#[tokio::test]
async fn deferred_expression_resolves_to_the_same_list() -> Result<()> {
    use vantage_expressions::traits::datasource::ExprDataSource;

    let csv = csv();
    let table = Table::<Csv, EmptyEntity>::new("bakery", csv.clone());
    let name_column = csv.create_column::<String>("name");
    let names = csv.column_table_values_expr(&table, &name_column);

    let deferred = csv.defer(names.expression().clone());
    let resolved = deferred.call().await?;
    let ExpressiveEnum::Scalar(list) = resolved else {
        panic!("deferred execution should resolve to a scalar");
    };
    let items = list.try_get::<Vec<AnyCsvType>>().expect("a list value");
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].value(), "Hill Valley Bakery");
    Ok(())
}

#[tokio::test]
async fn unsupported_condition_operator_is_an_error() {
    let mut table =
        Table::<Csv, EmptyEntity>::new("client", csv()).with_column_of::<String>("name");
    table.add_condition(Expression::new(
        "{} > {}",
        vec![
            ExpressiveEnum::Nested(table["name"].expr()),
            ExpressiveEnum::Nested(AnyCsvType::new("A".to_string()).expr()),
        ],
    ));

    assert!(table.list_values().await.is_err());
}
