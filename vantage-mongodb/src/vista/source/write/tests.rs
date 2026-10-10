use bson::doc;
use bson::oid::ObjectId;
use vantage_vista::VistaCapabilities;

use super::*;

#[test]
fn insert_at_path_builds_nested_doc() {
    let mut doc = Document::new();
    insert_at_path(
        &mut doc,
        &["address".to_string(), "city".to_string()],
        Bson::String("NYC".into()),
    );
    let address = doc.get_document("address").unwrap();
    assert_eq!(address.get_str("city").unwrap(), "NYC");
}

#[test]
fn insert_at_path_merges_siblings() {
    let mut doc = Document::new();
    insert_at_path(
        &mut doc,
        &["address".to_string(), "city".to_string()],
        Bson::String("NYC".into()),
    );
    insert_at_path(
        &mut doc,
        &["address".to_string(), "zip".to_string()],
        Bson::String("10001".into()),
    );
    let address = doc.get_document("address").unwrap();
    assert_eq!(address.get_str("city").unwrap(), "NYC");
    assert_eq!(address.get_str("zip").unwrap(), "10001");
}

#[test]
fn insert_at_path_overwrites_scalar_with_nested() {
    let mut doc = Document::new();
    doc.insert("address", "literal");
    insert_at_path(
        &mut doc,
        &["address".to_string(), "city".to_string()],
        Bson::String("NYC".into()),
    );
    let address = doc.get_document("address").unwrap();
    assert_eq!(address.get_str("city").unwrap(), "NYC");
}

/// A shell over a spec. The client connects lazily and these tests never
/// issue a command, so no server is needed.
async fn spec_shell(yaml: &str) -> MongoTableShell {
    let client = mongodb::Client::with_uri_str("mongodb://localhost:27017")
        .await
        .unwrap();
    let db = MongoDB::new(client, "unit");
    let spec: crate::vista::spec::MongoVistaSpec = serde_yaml_ng::from_str(yaml).unwrap();
    let table = crate::vista::factory::build_mongo_table(&spec, db, None).unwrap();
    let paths = crate::vista::factory::paths_from_table_columns(&table);
    let metadata = table.vista_metadata(Default::default());
    MongoTableShell::new(
        table,
        VistaCapabilities::default(),
        metadata,
        paths,
        None,
        false,
        None,
    )
}

const EVENT: &str = r#"
name: event
columns:
  _id: { type: object_id, flags: [id] }
  venue: { type: object_id, mongo: { nested_path: place.venue } }
  when: { type: datetime, mongo: { nested_path: place.when } }
  host: { mongo: { nested_path: place.host } }
  label: { mongo: { nested_path: place.label } }
"#;

fn text(s: &str) -> CborValue {
    CborValue::Text(s.to_string())
}

fn place(flat: &Record<AnyMongoType>) -> Document {
    flat["place"].value().as_document().unwrap().clone()
}

#[tokio::test]
async fn flatten_for_write_types_nested_paths() {
    let shell = spec_shell(EVENT).await;
    let (venue, host) = (ObjectId::new(), ObjectId::new());
    let mut record = Record::new();
    record.insert("venue".to_string(), text(&venue.to_hex()));
    record.insert("when".to_string(), text("2026-10-10T12:00:00Z"));
    record.insert("host".to_string(), text(&host.to_hex()));
    record.insert("label".to_string(), text(&host.to_hex()));

    let mut stored = IndexMap::new();
    stored.insert(
        "place".to_string(),
        Bson::Document(doc! { "host": host, "label": "hall" }),
    );
    let flat = shell.flatten_for_write(&record, Some(&stored)).unwrap();
    assert_eq!(
        place(&flat),
        doc! {
            "venue": venue,
            "when": bson::DateTime::parse_rfc3339_str("2026-10-10T12:00:00Z").unwrap(),
            "host": host,
            "label": host.to_hex(),
        }
    );

    // Without a stored row only declared columns are typed.
    let flat = shell.flatten_for_write(&record, None).unwrap();
    let typed = place(&flat);
    assert_eq!(typed.get("venue"), Some(&Bson::ObjectId(venue)));
    assert_eq!(typed.get("host"), Some(&Bson::String(host.to_hex())));

    record.insert("when".to_string(), text("later"));
    let err = shell.flatten_for_write(&record, None).unwrap_err();
    assert!(err.to_string().contains("when"), "{err}");
}

#[tokio::test]
async fn flatten_for_write_keeps_unchanged_stored_values() {
    let shell = spec_shell(EVENT).await;
    // Data that does not fit the declared types, read back unchanged.
    let far = bson::DateTime::from_millis(i64::MAX);
    let mut stored = IndexMap::new();
    stored.insert(
        "place".to_string(),
        Bson::Document(doc! { "venue": "c1", "when": far }),
    );
    let mut record = Record::new();
    record.insert("venue".to_string(), text("c1"));
    record.insert("when".to_string(), bson_to_cbor(&Bson::DateTime(far)));
    let flat = shell.flatten_for_write(&record, Some(&stored)).unwrap();
    assert_eq!(place(&flat), doc! { "venue": "c1", "when": far });

    // A changed value is checked against the declared type again; `""`
    // clears a declared column.
    record.insert("venue".to_string(), text("c2"));
    assert!(shell.flatten_for_write(&record, Some(&stored)).is_err());
    record.insert("venue".to_string(), text(""));
    let flat = shell.flatten_for_write(&record, Some(&stored)).unwrap();
    assert_eq!(place(&flat).get("venue"), Some(&Bson::Null));
}
