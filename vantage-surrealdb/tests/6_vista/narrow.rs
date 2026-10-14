//! Narrowing a vista: conditions (including on an `expr:` column), order and
//! search.

use ciborium::Value as CborValue;
use vantage_dataset::prelude::*;
#[cfg(feature = "rhai")]
use vantage_expressions::ExprDataSource;
#[cfg(feature = "rhai")]
use vantage_surrealdb::surreal_expr;
#[cfg(feature = "rhai")]
use vantage_types::Record;
#[cfg(feature = "rhai")]
use vantage_vista::VistaFactory;
use vantage_vista::{Capability, FilterOp, SortDirection};

use super::{TestResult, name_of, product_table, seed_row, setup};

#[tokio::test]
async fn vista_count_with_eq_condition() -> TestResult {
    let (db, table_name) = setup().await;
    seed_row(&db, &table_name, "a", "A", 10, false).await?;
    seed_row(&db, &table_name, "b", "B", 20, true).await?;
    seed_row(&db, &table_name, "c", "C", 30, false).await?;

    let table = product_table(db.clone(), &table_name);
    let mut vista = db.vista_factory().from_table(table)?;

    assert_eq!(vista.count().await?, 3);

    vista.add_condition("is_deleted", FilterOp::Eq, CborValue::Bool(false))?;
    assert_eq!(vista.count().await?, 2);

    let rows = vista.list_values().await?;
    assert_eq!(rows.len(), 2);

    Ok(())
}

#[tokio::test]
async fn vista_eq_condition_with_typed_value() -> TestResult {
    let (db, table_name) = setup().await;
    seed_row(&db, &table_name, "a", "Match", 10, false).await?;
    seed_row(&db, &table_name, "b", "Other", 20, false).await?;

    let table = product_table(db.clone(), &table_name);
    let mut vista = db.vista_factory().from_table(table)?;

    vista.add_condition("name", FilterOp::Eq, CborValue::Text("Match".into()))?;
    let rows = vista.list_values().await?;
    assert_eq!(rows.len(), 1);
    let (_, row) = rows.into_iter().next().unwrap();
    assert_eq!(row.get("name"), Some(&CborValue::Text("Match".into())));

    Ok(())
}

#[tokio::test]
async fn vista_unknown_field_eq_errors() -> TestResult {
    let (db, table_name) = setup().await;
    let table = product_table(db.clone(), &table_name);
    let mut vista = db.vista_factory().from_table(table)?;

    let err = vista
        .add_condition("nonexistent", FilterOp::Eq, CborValue::Bool(true))
        .expect_err("should reject unknown field");
    assert!(err.to_string().contains("nonexistent"));

    Ok(())
}

#[tokio::test]
async fn vista_set_order_filters_results_with_replace_semantics() -> TestResult {
    let (db, table_name) = setup().await;
    seed_row(&db, &table_name, "beta", "Beta", 20, false).await?;
    seed_row(&db, &table_name, "alpha", "Alpha", 10, false).await?;
    seed_row(&db, &table_name, "gamma", "Gamma", 30, false).await?;

    let table = product_table(db.clone(), &table_name);
    let mut vista = db.vista_factory().from_table(table)?;
    assert!(vista.capabilities().has(Capability::Order));

    vista.set_order(Some(("price", SortDirection::Ascending)))?;
    let rows = vista.list_values().await?;
    let names: Vec<String> = rows.values().map(name_of).collect();
    assert_eq!(
        names,
        vec!["Alpha".to_string(), "Beta".into(), "Gamma".into()]
    );

    vista.set_order(Some(("name", SortDirection::Descending)))?;
    let rows = vista.list_values().await?;
    let names: Vec<String> = rows.values().map(name_of).collect();
    assert_eq!(
        names,
        vec!["Gamma".to_string(), "Beta".into(), "Alpha".into()]
    );

    vista.set_order(None)?;
    let _rows = vista.list_values().await?;
    Ok(())
}

#[tokio::test]
async fn vista_set_search_uses_string_contains() -> TestResult {
    let (db, table_name) = setup().await;
    seed_row(&db, &table_name, "alpha", "Alpha", 10, false).await?;
    seed_row(&db, &table_name, "beta", "Beta", 20, false).await?;
    seed_row(&db, &table_name, "gamma", "Gamma", 30, false).await?;

    let table = product_table(db.clone(), &table_name);
    let mut vista = db.vista_factory().from_table(table)?;
    assert!(vista.capabilities().has(Capability::Search));

    vista.set_search(Some("amma"))?;
    let rows = vista.list_values().await?;
    assert_eq!(
        rows.len(),
        1,
        "only Gamma contains 'amma' (case-insensitive)"
    );

    vista.set_search(Some("alpha"))?;
    let rows = vista.list_values().await?;
    assert_eq!(rows.len(), 1);

    vista.set_search(None)?;
    let rows = vista.list_values().await?;
    assert_eq!(rows.len(), 3);
    Ok(())
}

/// An `expr:` column is projected as `(<expr>) AS <name>`, but SurrealDB
/// evaluates WHERE before the projection, so a condition has to carry the
/// expression itself. Each test runs in its own random database, so the
/// seeded rows need no cleanup.
#[cfg(feature = "rhai")]
#[tokio::test]
async fn vista_filters_by_expr_column_on_the_expression() -> TestResult {
    let (db, table_name) = setup().await;
    for (id, qty, price) in [("big", 2i64, 5i64), ("small", 1, 3)] {
        db.execute(&surreal_expr!(
            &format!("CREATE {table_name}:{id} SET qty = {{}}, price = {{}}"),
            qty,
            price
        ))
        .await?;
    }

    let yaml = format!(
        r#"
name: line_view
columns:
  id: {{ type: thing, flags: [id] }}
  qty: {{ type: int }}
  price: {{ type: int }}
  total: {{ type: int, expr: 'surreal::mul(surreal::ident("qty"), surreal::ident("price"))' }}
  label:
    type: string
    expr: 'surreal::expr("string::concat(\"item-\", <string>qty)")'
surreal:
  table: {table_name}
"#
    );
    let vista = || db.vista_factory().from_yaml(&yaml);
    let ids = |rows: indexmap::IndexMap<String, Record<CborValue>>| -> Vec<String> {
        rows.into_keys().collect()
    };
    let big = format!("{table_name}:big");
    let small = format!("{table_name}:small");

    let mut gt = vista()?;
    gt.add_condition("total", FilterOp::Gt, CborValue::Integer(6.into()))?;
    assert_eq!(ids(gt.list_values().await?), vec![big.clone()]);

    let mut eq = vista()?;
    eq.add_condition("total", FilterOp::Eq, CborValue::Integer(3.into()))?;
    assert_eq!(ids(eq.list_values().await?), vec![small.clone()]);

    let mut like = vista()?;
    like.add_condition("label", FilterOp::Like, CborValue::Text("ITEM-2".into()))?;
    assert_eq!(ids(like.list_values().await?), vec![big.clone()]);

    let mut sorted = vista()?;
    sorted.set_order(Some(("total", SortDirection::Ascending)))?;
    assert_eq!(ids(sorted.list_values().await?), vec![small, big.clone()]);

    let mut searched = vista()?;
    searched.set_search(Some("item-2"))?;
    assert_eq!(ids(searched.list_values().await?), vec![big]);
    Ok(())
}
