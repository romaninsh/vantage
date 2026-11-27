//! Translated columns: a `tag` whose JSON `name` holds a name per language,
//! a nested column per language, and `name_local`, an `expr:` coalescing
//! them through `table.column`, read on its own and through a relation.
//! `vista_tests!` invokes it with the spec type and YAML driver key; the
//! tables live in the database `clients_orders` connects to.

macro_rules! vista_translated_tests {
    ($spec:ty, $block:literal) => {
        #[cfg(feature = "rhai")]
        mod translated {
            use ciborium::Value as CborValue;
            use vantage_dataset::prelude::*;
            use vantage_vista::{FilterOp, SortDirection, Vista, VistaFactory};

            #[allow(unused_imports)]
            use super::*;
            use super::{TestResult, clients_orders, spec_resolver};

            /// `tag` and `todo` tables under `suffix`: todo `d1` has tags
            /// `t1` (en_US "Defect", de_DE "Fehler") and `t2` (en_US "Bug"
            /// only); todo `d2` has none.
            async fn tags_todos(suffix: &str) -> (Vista, Vista) {
                let (db, _, _) = clients_orders(suffix).await;
                let block = $block;
                let json = match block {
                    "postgres" => "JSONB",
                    "mysql" => "JSON",
                    _ => "TEXT",
                };
                let (tag_t, todo_t) = (format!("tr_tag_{suffix}"), format!("tr_todo_{suffix}"));
                for sql in [
                    format!("DROP TABLE IF EXISTS {tag_t}"),
                    format!("DROP TABLE IF EXISTS {todo_t}"),
                    format!(
                        "CREATE TABLE {tag_t} (id VARCHAR(20) PRIMARY KEY, todo_id VARCHAR(20), name {json})"
                    ),
                    format!("CREATE TABLE {todo_t} (id VARCHAR(20) PRIMARY KEY)"),
                    format!(
                        r#"INSERT INTO {tag_t} VALUES ('t1', 'd1', '{{"en_US":"Defect","de_DE":"Fehler"}}'), ('t2', 'd1', '{{"en_US":"Bug"}}')"#
                    ),
                    format!("INSERT INTO {todo_t} VALUES ('d1'), ('d2')"),
                ] {
                    sqlx::query(sqlx::AssertSqlSafe(sql))
                        .execute(db.pool())
                        .await
                        .unwrap();
                }
                let tag_spec: $spec = serde_yaml_ng::from_str(&format!(
                    r#"
name: tag
columns:
  id: {{ type: string, flags: [id] }}
  todo_id: {{ type: string }}
  name: {{ type: json }}
  name_en: {{ type: string, nested_path: name.en_US }}
  name_de: {{ type: string, nested_path: name.de_DE }}
  name_local: {{ type: string, flags: [searchable], expr: 'sql::coalesce(table.column("name_de"), table.column("name_en"))' }}
{block}:
  table: {tag_t}
"#
                ))
                .unwrap();
                let todo_spec: $spec = serde_yaml_ng::from_str(&format!(
                    r#"
name: todo
columns:
  id: {{ type: string, flags: [id] }}
  tag_names: {{ type: string, expr: 'table.ref("tags").order_by("name_local").join_concat("name_local", ", ")' }}
{block}:
  table: {todo_t}
references:
  tags:
    table: tag
    kind: has_many
    foreign_key: todo_id
"#
                ))
                .unwrap();
                let factory = db
                    .vista_factory()
                    .with_resolver(spec_resolver(vec![("tag", tag_spec.clone())]));
                let tags = factory.build_from_spec(tag_spec).expect("tag loads");
                let todos = factory.build_from_spec(todo_spec).expect("todo loads");
                (tags, todos)
            }

            fn text(s: &str) -> CborValue {
                CborValue::Text(s.into())
            }

            /// `name_local` reads the German name, else the English one.
            #[tokio::test]
            async fn name_local_falls_back() -> TestResult {
                let (tags, _) = tags_todos("falls_back").await;
                let rows = tags.list_values().await?;
                assert_eq!(rows["t1"]["name_local"], text("Fehler"));
                assert_eq!(rows["t2"]["name_local"], text("Bug"));
                Ok(())
            }

            /// A relation's `join_concat` over the related `name_local`
            /// inlines its expression, and orders by it.
            #[tokio::test]
            async fn join_concat_related_name_local() -> TestResult {
                let (_, todos) = tags_todos("join_local").await;
                let rows = todos.list_values().await?;
                assert_eq!(rows["d1"]["tag_names"], text("Bug, Fehler"));
                assert_eq!(rows["d2"]["tag_names"], text(""));
                Ok(())
            }

            /// A condition, a search and a sort on `name_local` render its
            /// COALESCE: SQL has no SELECT alias in WHERE, and ORDER BY
            /// can't count on one.
            #[tokio::test]
            async fn no_alias_in_where_or_order() -> TestResult {
                let (tags, _) = tags_todos("no_alias").await;
                let narrowed = tags
                    .clone()
                    .with_condition("name_local", FilterOp::Eq, text("Fehler"))?
                    .with_search("Feh")?
                    .with_order("name_local", SortDirection::Ascending)?;
                let query = narrowed.preview().query.unwrap_or_default();
                let (_, after) = query.split_once(" FROM ").expect("a FROM");
                let (filter, order) = after.split_once("ORDER BY").expect("an ORDER BY");
                for part in [filter, order] {
                    assert!(part.to_uppercase().contains("COALESCE"), "{query}");
                    assert!(!part.contains("name_local"), "{query}");
                }
                let rows = narrowed.list_values().await?;
                assert_eq!(rows.keys().collect::<Vec<_>>(), ["t1"], "{query}");

                let sorted = tags.with_order("name_local", SortDirection::Ascending)?;
                let ids: Vec<String> = sorted.list_values().await?.into_keys().collect();
                assert_eq!(ids, ["t2", "t1"]);
                Ok(())
            }
        }
    };
}
