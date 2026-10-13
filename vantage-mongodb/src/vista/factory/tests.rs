use super::build::missing_rhai;
#[cfg(not(feature = "rhai"))]
use super::*;

#[test]
fn missing_rhai_names_the_feature_and_slot() {
    let msg = missing_rhai("modify").to_string();
    assert!(msg.contains("`rhai` feature"), "{msg}");
    assert!(msg.contains("modify"), "{msg}");
}

#[cfg(not(feature = "rhai"))]
#[tokio::test]
async fn reference_scripts_without_rhai_fail_the_build() {
    use vantage_vista::VistaFactory;

    let client = mongodb::Client::with_uri_str("mongodb://localhost:27017")
        .await
        .unwrap();
    let spec: MongoVistaSpec = serde_yaml_ng::from_str(
        r#"
name: client
columns:
  _id: { type: string, flags: [id] }
references:
  orders:
    table: order
    kind: has_many
    foreign_key: client_id
    mongo: { rhai: 'table("order")' }
"#,
    )
    .unwrap();
    let err = MongoDB::new(client, "unit")
        .vista_factory()
        .build_from_spec(spec)
        .expect_err("a reference script needs the rhai feature");
    assert!(err.to_string().contains("`rhai` feature"), "{err}");
    assert_eq!(
        err.context.get("vista").map(String::as_str),
        Some("\"client\"")
    );
    assert_eq!(
        err.context.get("relation").map(String::as_str),
        Some("\"orders\"")
    );
}
