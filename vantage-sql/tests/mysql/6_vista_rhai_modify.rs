//! `mysql: { modify }` scripts and the `with_condition` verb: the script runs
//! against the built vista as `self` and narrows it with SQL expressions from
//! the MySQL Rhai vocabulary.
//!
//! Requires a running MySQL on `mysql://vantage:vantage@localhost:3306/vantage`.

use std::error::Error;

use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_dataset::prelude::*;
use vantage_types::Record;
use vantage_vista::{Capability, NativeCondition, VistaFactory};

use fixture::{Fixture, order, sorted_ids, spec, stored_orders};

#[path = "6_vista_rhai_modify/fixture.rs"]
mod fixture;

type TestResult = std::result::Result<(), Box<dyn Error>>;

#[tokio::test]
async fn modify_script_adds_sql_condition() -> TestResult {
    let f = Fixture::new("cond").await;
    let block = format!(
        "  table: {}\n  modify: |\n    self.with_condition(sql::ident(\"total\") > 15)",
        f.orders_t
    );
    let orders =
        f.db.vista_factory()
            .build_from_spec(f.orders_spec(&block))?;
    assert_eq!(sorted_ids(&orders.list_values().await?), vec!["o2", "o3"]);
    assert!(
        orders.capabilities().has(Capability::Insert),
        "a modified table stays writable"
    );
    // The script's condition is recorded by its rendered SQL.
    assert_eq!(orders.query().native.len(), 1);
    assert!(orders.query().native[0].contains("total"));

    f.drop_tables().await;
    Ok(())
}

#[tokio::test]
async fn modify_script_composes_with_rhai_source() -> TestResult {
    let f = Fixture::new("rhai").await;
    let block = format!(
        "  rhai: |\n    sql::select().from(\"{}\").field(\"id\").field(\"client_id\").field(\"total\")\n  \
         modify: |\n    self.with_condition(sql::ident(\"client_id\") == \"alice\")",
        f.orders_t
    );
    let orders =
        f.db.vista_factory()
            .build_from_spec(f.orders_spec(&block))?;
    assert!(
        !orders.capabilities().has(Capability::Insert),
        "query-sourced vista is read-only"
    );
    assert_eq!(sorted_ids(&orders.list_values().await?), vec!["o1", "o2"]);

    f.drop_tables().await;
    Ok(())
}

#[tokio::test]
async fn modify_script_composes_with_base() -> TestResult {
    let f = Fixture::new("base").await;
    let derived = spec(
        r#"
name: big_orders
columns:
  id: { type: string, flags: [id] }
  total: { type: int }
mysql:
  base: orders
  inherit:
    columns: [id, total]
  modify: |
    self.with_condition(sql::ident("total") >= 20)
"#,
    );
    let factory = f.db.vista_factory().with_resolver(f.resolver());
    let big = factory.build_from_spec(derived)?;
    assert_eq!(sorted_ids(&big.list_values().await?), vec!["o2", "o3"]);

    f.drop_tables().await;
    Ok(())
}

#[tokio::test]
async fn modify_script_table_resolves_through_resolver() -> TestResult {
    let f = Fixture::new("table").await;
    let block = format!(
        "  table: {}\n  modify: |\n    table(\"orders\").where(\"client_id\", \"bob\")",
        f.orders_t
    );
    let factory = f.db.vista_factory().with_resolver(f.resolver());
    let orders = factory.build_from_spec(f.orders_spec(&block))?;
    assert_eq!(sorted_ids(&orders.list_values().await?), vec!["o3"]);

    f.drop_tables().await;
    Ok(())
}

/// `table(name)` in a `modify:` script builds through the factory, so the
/// factory's injector narrows it: bob's `o3` is outside the injected `o1`.
#[tokio::test]
async fn injector_condition_applies_to_a_modify_script_table() -> TestResult {
    use vantage_sql::mysql::{AnyMysqlType, MysqlDB};
    use vantage_table::table::TableInjector;
    use vantage_table::traits::table_source::TableSource;

    let f = Fixture::new("modinject").await;
    let only_o1: TableInjector<MysqlDB> = std::sync::Arc::new(|cx, table| {
        if cx.key == "orders" {
            let o1 = table
                .data_source()
                .eq_value_condition("id", AnyMysqlType::from("o1"))?;
            table.add_condition(o1);
        }
        Ok(())
    });
    let bob_orders = spec(&format!(
        r#"
name: bob_orders
columns:
  id: {{ type: string, flags: [id] }}
  client_id: {{ type: string }}
  total: {{ type: int }}
mysql:
  table: {}
  modify: |
    table("orders").where("client_id", "bob")
"#,
        f.orders_t
    ));
    let factory =
        f.db.vista_factory()
            .with_resolver(f.resolver())
            .with_injector(only_o1);
    let orders = factory.build_from_spec(bob_orders)?;
    assert!(orders.list_values().await?.is_empty());

    f.drop_tables().await;
    Ok(())
}

#[tokio::test]
async fn native_condition_rejects_foreign_type() -> TestResult {
    let f = Fixture::new("raw").await;
    let mut orders =
        f.db.vista_factory()
            .build_from_spec(f.plain_orders_spec())?;
    let err = orders
        .add_native_condition(NativeCondition::new(42_i32, "42"))
        .expect_err("an i32 is not a MySQL condition");
    assert!(err.is_incorrect_usage(), "a wrong type is a caller mistake");
    assert!(err.to_string().contains("native condition"), "{err}");
    assert!(
        orders.query().native.is_empty(),
        "a refused condition isn't recorded"
    );

    f.drop_tables().await;
    Ok(())
}

#[tokio::test]
async fn native_condition_accepts_mysql_expression() -> TestResult {
    let f = Fixture::new("rawok").await;
    let mut orders =
        f.db.vista_factory()
            .build_from_spec(f.plain_orders_spec())?;
    let expr = vantage_sql::mysql_expr!("total < {}", 15_i64);
    let key = expr.preview();
    orders.add_native_condition(NativeCondition::new(expr, key.clone()))?;
    assert_eq!(orders.query().native, vec![key]);
    assert_eq!(sorted_ids(&orders.list_values().await?), vec!["o1"]);

    f.drop_tables().await;
    Ok(())
}

/// `with_condition` only narrows `self`; on a `table(...)` handle it is
/// refused, as on every backend.
#[tokio::test]
async fn scripted_ref_refuses_with_condition_on_table_handle() -> TestResult {
    let f = Fixture::new("refcond").await;
    let client = spec(
        r#"
name: client
columns:
  id: { type: string, flags: [id] }
mysql:
  table: unused_client
references:
  orders:
    table: orders
    kind: has_many
    foreign_key: client_id
    mysql:
      rhai: |
        table("orders").with_condition(sql::ident("total") > 15)
"#,
    );
    let factory = f.db.vista_factory().with_resolver(f.resolver());
    let clients = factory.build_from_spec(client)?;
    let mut row = Record::new();
    row.insert("id".to_string(), CborValue::Text("alice".into()));
    let err = clients
        .get_ref("orders", Some(&row))
        .map(drop)
        .expect_err("with_condition on table(...) must fail");
    assert!(
        err.to_string().contains(r#"verb: "with_condition""#),
        "{err}"
    );

    f.drop_tables().await;
    Ok(())
}

/// Writes through a `modify:`-narrowed vista keep the narrowing: a row held
/// outside the condition, or a new row that does not fit it, is a `Conflict`
/// and nothing is written.
#[tokio::test]
async fn modify_script_condition_guards_writes() -> TestResult {
    let f = Fixture::new("guard").await;
    sqlx::query(&format!("DELETE FROM `{}`", f.orders_t))
        .execute(f.db.pool())
        .await?;
    sqlx::query(&format!(
        "INSERT INTO `{}` VALUES ('o1','alice',10)",
        f.orders_t
    ))
    .execute(f.db.pool())
    .await?;
    let before = stored_orders(&f).await;

    let block = format!(
        "  table: {}\n  modify: |\n    self.with_condition(sql::ident(\"total\") > 15)",
        f.orders_t
    );
    let orders =
        f.db.vista_factory()
            .build_from_spec(f.orders_spec(&block))?;

    let held: IndexMap<String, Record<CborValue>> = [("o1".to_string(), order("alice", 20))]
        .into_iter()
        .collect();
    let Err(err) = orders.import_values(&held).await else {
        panic!("an id held outside the condition must conflict");
    };
    assert!(err.is_conflict(), "wrong error kind: {err}");
    assert_eq!(stored_orders(&f).await, before);

    let misfit: IndexMap<String, Record<CborValue>> = [("o2".to_string(), order("alice", 5))]
        .into_iter()
        .collect();
    let Err(err) = orders.import_values(&misfit).await else {
        panic!("a row outside the condition must conflict");
    };
    assert!(err.is_conflict(), "wrong error kind: {err}");
    assert_eq!(stored_orders(&f).await, before);

    let Err(err) = orders
        .insert_value("o3".to_string(), &order("alice", 5))
        .await
    else {
        panic!("an insert outside the condition must conflict");
    };
    assert!(err.is_conflict(), "wrong error kind: {err}");
    assert_eq!(stored_orders(&f).await, before);

    let fits: IndexMap<String, Record<CborValue>> = [("o4".to_string(), order("alice", 20))]
        .into_iter()
        .collect();
    assert_eq!(orders.import_values(&fits).await?, 1);
    assert_eq!(
        stored_orders(&f).await,
        vec![("o1".to_string(), 10), ("o4".to_string(), 20)]
    );

    f.drop_tables().await;
    Ok(())
}
