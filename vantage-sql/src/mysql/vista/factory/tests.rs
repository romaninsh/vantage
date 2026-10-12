use sqlx::mysql::MySqlPoolOptions;
use vantage_vista::VistaFactory;

use super::*;

/// A pool that never connects: the specs below fail before any query.
fn factory() -> MysqlVistaFactory {
    let pool = MySqlPoolOptions::new()
        .connect_lazy("mysql://user:pass@localhost/none")
        .unwrap();
    MysqlVistaFactory::new(MysqlDB { pool })
}

fn spec(yaml: &str) -> MysqlVistaSpec {
    serde_yaml_ng::from_str(yaml).unwrap()
}

#[tokio::test]
async fn scripts_without_rhai_fail_the_build() {
    let with_ref_script = spec(
        r#"
name: bakery
columns:
  id: { type: int, flags: [id] }
references:
  products:
    table: product
    kind: has_many
    foreign_key: bakery_id
    mysql:
      rhai: 'table("product")'
"#,
    );
    let err = factory().build_from_spec(with_ref_script).unwrap_err();
    assert_eq!(
        err.context.get("vista").map(String::as_str),
        Some("\"bakery\"")
    );
    assert_eq!(
        err.context.get("relation").map(String::as_str),
        Some("\"products\"")
    );

    let with_modify = spec(
        r#"
name: bakery
columns:
  id: { type: int, flags: [id] }
mysql:
  modify: 'self'
"#,
    );
    let err = factory().build_from_spec(with_modify).unwrap_err();
    assert_eq!(
        err.context.get("vista").map(String::as_str),
        Some("\"bakery\"")
    );
}
