//! The suite's product-table cases: reads, conditions, YAML loading,
//! writes and a Rhai-built source. `vista_tests!` invokes it with the
//! dialect's YAML driver key.

macro_rules! vista_product_tests {
    ($block:literal) => {
        #[tokio::test]
        async fn vista_get_value_by_id() -> TestResult {
            let (db, table) = product("get").await;
            let vista = db.vista_factory().from_table(table)?;

            let row = vista.get_value("b").await?.expect("row b exists");
            assert_eq!(
                row.get("name"),
                Some(&ciborium::Value::Text("Beta".to_string()))
            );

            let missing = vista.get_value("nope").await?;
            assert!(missing.is_none());
            Ok(())
        }

        #[tokio::test]
        async fn vista_count_with_eq_condition() -> TestResult {
            let (db, table) = product("count").await;
            let mut vista = db.vista_factory().from_table(table)?;

            assert_eq!(vista.count().await?, 3);

            vista.add_condition(
                "is_deleted",
                vantage_vista::FilterOp::Eq,
                ciborium::Value::Bool(false),
            )?;
            assert_eq!(vista.count().await?, 2);

            let rows = vista.list_values().await?;
            assert_eq!(rows.len(), 2);
            assert!(rows.contains_key("a"));
            assert!(rows.contains_key("c"));
            assert!(!rows.contains_key("b"));
            Ok(())
        }

        /// SQL compares whole column values, so a path into one is refused as
        /// `Unimplemented`, and `with_query` hands it back as residual.
        #[tokio::test]
        async fn vista_condition_on_a_dotted_path_is_refused() -> TestResult {
            let (db, table) = product("dotted").await;
            let vista = db.vista_factory().from_table(table)?;
            crate::dotted::assert_dotted_path_is_refused(&vista, "name.first");
            Ok(())
        }

        #[tokio::test]
        async fn vista_yaml_loads_table_and_columns() -> TestResult {
            let (db, table) = product("yaml").await;
            let (block, name) = ($block, table.table_name());

            let yaml = format!(
                r#"
name: product_view
columns:
  id:
    type: string
    flags: [id]
  name:
    type: string
    flags: [title, searchable]
  price:
    type: int
{block}:
  table: {name}
"#
            );

            let vista = db.vista_factory().from_yaml(&yaml)?;

            assert_eq!(vista.name(), "product_view");
            assert_eq!(vista.id_column(), Some("id"));
            assert_eq!(vista.title_columns(), vec!["name"]);

            let rows = vista.list_values().await?;
            assert_eq!(rows.len(), 3);
            assert!(rows.contains_key("a"));
            Ok(())
        }

        #[tokio::test]
        async fn vista_writes_round_trip_via_cbor() -> TestResult {
            let (db, table) = product("write").await;
            let vista = db.vista_factory().from_table(table)?;

            let record: vantage_types::Record<ciborium::Value> = vec![
                ("name".to_string(), ciborium::Value::Text("Delta".into())),
                ("price".to_string(), ciborium::Value::Integer(99i64.into())),
                ("is_deleted".to_string(), ciborium::Value::Bool(false)),
            ]
            .into_iter()
            .collect();

            vista.insert_value("d", &record).await?;

            let fetched = vista.get_value("d").await?.expect("inserted");
            assert_eq!(
                fetched.get("name"),
                Some(&ciborium::Value::Text("Delta".into()))
            );

            vista.delete("d").await?;
            assert!(vista.get_value("d").await?.is_none());
            Ok(())
        }

        /// A vista whose source is a Rhai-built SELECT (not a physical table).
        /// The script filters `price > 15`, so only two products surface, and
        /// the vista is read-only.
        #[cfg(feature = "rhai")]
        #[tokio::test]
        async fn vista_yaml_rhai_source_is_read_only_and_filters() -> TestResult {
            use vantage_vista::Capability;

            let (db, table) = product("rhai").await;
            let (block, name) = ($block, table.table_name());

            let yaml = format!(
                r#"
name: expensive_products
columns:
  id:
    type: string
    flags: [id]
  name:
    type: string
{block}:
  rhai: |
    sql::select().from("{name}").field("id").field("name").where(sql::expr("price > 15"))
"#
            );

            let vista = db.vista_factory().from_yaml(&yaml)?;
            assert_eq!(vista.name(), "expensive_products");

            let caps = vista.capabilities();
            assert!(
                !caps.has(Capability::Insert),
                "rhai-sourced vista is read-only"
            );
            assert!(!caps.has(Capability::Update));
            assert!(!caps.has(Capability::Delete));
            assert!(!caps.has(Capability::ConfineWrites));
            assert!(!caps.has(Capability::Import));

            let rows = vista.list_values().await?;
            assert_eq!(rows.len(), 2, "only products with price > 15");
            assert!(rows.contains_key("b"));
            assert!(rows.contains_key("c"));
            assert!(!rows.contains_key("a"));
            Ok(())
        }
    };
}
