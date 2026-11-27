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

            /// Each nested column writes its own key of `doc`. A key with `@`
            /// (`sr@latin`) works like any other, a key the host lacks is added,
            /// `""` is a value, and a saved null removes only its own key.
            #[tokio::test]
            async fn nested_columns_write_one_key_each() -> TestResult {
                let vista = tags("nested_keys").await;
                vista
                    .insert_value(
                        "t1",
                        &row(&[("name_en", text("Bug")), ("name_sr", text("Greška"))]),
                    )
                    .await?;
                assert_eq!(read(&vista, "t1", "name_sr").await, text("Greška"));
                assert_eq!(read(&vista, "t1", "name_de").await, CborValue::Null);

                vista
                    .patch_value(
                        "t1",
                        &row(&[("name_sr", text("Bag")), ("name_de", text("Fehler"))]),
                    )
                    .await?;
                assert_eq!(
                    doc(&vista, "t1").await,
                    serde_json::json!({ "en_US": "Bug", "sr@latin": "Bag", "de_DE": "Fehler" })
                );

                vista
                    .patch_value("t1", &row(&[("name_de", text(""))]))
                    .await?;
                assert_eq!(read(&vista, "t1", "name_de").await, text(""));

                vista
                    .patch_value("t1", &row(&[("name_de", CborValue::Null)]))
                    .await?;
                assert_eq!(
                    doc(&vista, "t1").await,
                    serde_json::json!({ "en_US": "Bug", "sr@latin": "Bag" })
                );
                assert_eq!(read(&vista, "t1", "name_de").await, CborValue::Null);
                Ok(())
            }

            /// The host itself: written whole, it keeps a key no nested column
            /// reads (it is a free-form map); stored NULL, the first nested
            /// write creates the JSON object.
            #[tokio::test]
            async fn the_host_is_a_free_form_map() -> TestResult {
                let vista = tags("nested_host").await;
                let whole = CborValue::Map(vec![
                    (text("en_US"), text("X")),
                    (text("fr_FR"), text("Y")),
                ]);
                vista.insert_value("t1", &row(&[("doc", whole.clone())])).await?;
                assert_eq!(read(&vista, "t1", "name_en").await, text("X"));
                vista
                    .insert_value("t3", &row(&[("name_en", text("Bug"))]))
                    .await?;
                vista.patch_value("t3", &row(&[("doc", whole)])).await?;
                assert_eq!(
                    doc(&vista, "t3").await,
                    serde_json::json!({ "en_US": "X", "fr_FR": "Y" })
                );

                vista
                    .insert_value("t2", &row(&[("doc", CborValue::Null)]))
                    .await?;
                vista
                    .patch_value("t2", &row(&[("name_de", text("Fehler"))]))
                    .await?;
                assert_eq!(
                    doc(&vista, "t2").await,
                    serde_json::json!({ "de_DE": "Fehler" })
                );
                Ok(())
            }
        }
    };
}
