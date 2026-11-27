//! Nested columns over one JSON host: `doc` of the `typed_events` table
//! (`vista/typed_writes.rs`) holds a translation per language, and each
//! nested column reads and writes one key of it. `vista_tests!` invokes it
//! with the dialect's YAML driver key.

macro_rules! vista_nested_writes_tests {
    ($block:literal) => {
        mod nested_writes {
            use ciborium::Value as CborValue;
            use vantage_dataset::prelude::*;
            use vantage_types::Record;
            use vantage_vista::{Vista, VistaFactory};

            use super::{TestResult, typed_events};

            async fn tags(suffix: &str) -> Vista {
                let (db, name) = typed_events(suffix).await;
                let block = $block;
                let yaml = format!(
                    r#"
name: {name}
columns:
  id: {{ type: string, flags: [id] }}
  doc: {{ type: json }}
  name_en: {{ type: string, nested_path: doc.en_US }}
  name_de: {{ type: string, nested_path: doc.de_DE }}
  name_sr: {{ type: string, nested_path: doc.sr@latin }}
{block}:
  table: {name}
"#
                );
                db.vista_factory()
                    .from_yaml(&yaml)
                    .expect("nested vista loads")
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

            /// The host as stored, as JSON: an object, or JSON text of one.
            async fn doc(vista: &Vista, id: &str) -> serde_json::Value {
                let stored = vista.get_value(id).await.unwrap().expect("row");
                match &stored["doc"] {
                    CborValue::Text(s) => serde_json::from_str(s).unwrap(),
                    other => serde_json::to_value(other).unwrap(),
                }
            }

            async fn read(vista: &Vista, id: &str, column: &str) -> CborValue {
                vista.get_value(id).await.unwrap().expect("row")[column].clone()
            }

            /// A key with `@` (`sr@latin`) reads and writes like any other.
            #[tokio::test]
            async fn nested_column_with_at_key() -> TestResult {
                let vista = tags("nested_at").await;
                vista
                    .insert_value(
                        "t1",
                        &row(&[("name_en", text("Bug")), ("name_sr", text("Greška"))]),
                    )
                    .await?;
                assert_eq!(read(&vista, "t1", "name_sr").await, text("Greška"));
                vista
                    .patch_value("t1", &row(&[("name_sr", text("Bag"))]))
                    .await?;
                assert_eq!(read(&vista, "t1", "name_sr").await, text("Bag"));
                assert_eq!(
                    doc(&vista, "t1").await,
                    serde_json::json!({ "en_US": "Bug", "sr@latin": "Bag" })
                );
                Ok(())
            }

            /// A saved null removes the key; the other keys stay.
            #[tokio::test]
            async fn null_removes_key() -> TestResult {
                let vista = tags("nested_null").await;
                vista
                    .insert_value(
                        "t1",
                        &row(&[("name_en", text("Bug")), ("name_de", text("Fehler"))]),
                    )
                    .await?;
                vista
                    .patch_value("t1", &row(&[("name_de", CborValue::Null)]))
                    .await?;
                assert_eq!(
                    doc(&vista, "t1").await,
                    serde_json::json!({ "en_US": "Bug" })
                );
                assert_eq!(read(&vista, "t1", "name_en").await, text("Bug"));
                assert_eq!(read(&vista, "t1", "name_de").await, CborValue::Null);
                Ok(())
            }

            /// A patch of a key the host doesn't hold yet adds it.
            #[tokio::test]
            async fn patch_creates_missing_key() -> TestResult {
                let vista = tags("nested_missing").await;
                vista
                    .insert_value("t1", &row(&[("name_en", text("Bug"))]))
                    .await?;
                assert_eq!(read(&vista, "t1", "name_de").await, CborValue::Null);
                vista
                    .patch_value("t1", &row(&[("name_de", text("Fehler"))]))
                    .await?;
                assert_eq!(read(&vista, "t1", "name_de").await, text("Fehler"));
                assert_eq!(read(&vista, "t1", "name_en").await, text("Bug"));

                // What if the host is written whole with a key no nested
                // column reads: the host is a free-form map, so it's kept.
                let whole = CborValue::Map(vec![
                    (text("en_US"), text("X")),
                    (text("fr_FR"), text("Y")),
                ]);
                vista
                    .patch_value("t1", &row(&[("doc", whole.clone())]))
                    .await?;
                assert_eq!(
                    doc(&vista, "t1").await,
                    serde_json::json!({ "en_US": "X", "fr_FR": "Y" })
                );
                vista.insert_value("t2", &row(&[("doc", whole)])).await?;
                assert_eq!(read(&vista, "t2", "name_en").await, text("X"));
                Ok(())
            }

            /// A patch on a NULL host creates the JSON object.
            #[tokio::test]
            async fn patch_on_null_host_creates_json() -> TestResult {
                let vista = tags("nested_nullhost").await;
                vista
                    .insert_value("t1", &row(&[("doc", CborValue::Null)]))
                    .await?;
                vista
                    .patch_value("t1", &row(&[("name_de", text("Fehler"))]))
                    .await?;
                assert_eq!(
                    doc(&vista, "t1").await,
                    serde_json::json!({ "de_DE": "Fehler" })
                );
                assert_eq!(read(&vista, "t1", "name_en").await, CborValue::Null);
                Ok(())
            }

            /// An empty string is a value: stored and read as `""`.
            #[tokio::test]
            async fn empty_string_kept() -> TestResult {
                let vista = tags("nested_empty").await;
                vista
                    .insert_value("t1", &row(&[("name_en", text("Bug"))]))
                    .await?;
                vista
                    .patch_value("t1", &row(&[("name_de", text(""))]))
                    .await?;
                assert_eq!(read(&vista, "t1", "name_de").await, text(""));
                assert_eq!(
                    doc(&vista, "t1").await,
                    serde_json::json!({ "en_US": "Bug", "de_DE": "" })
                );
                Ok(())
            }
        }
    };
}
