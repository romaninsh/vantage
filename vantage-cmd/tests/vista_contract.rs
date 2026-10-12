//! `vantage_vista::contract` over a local shell-script fixture. The backend
//! is read-only, so there are no write probes.

use ciborium::Value as CborValue;
use vantage_cmd::Cmd;
use vantage_table::table::Table;
use vantage_types::EmptyEntity;
use vantage_vista::{Vista, contract};

/// Rows `alpha` and `beta`, keyed by `name`.
fn items() -> Vista {
    let script = r#"
        let out = run([]);
        if out.exit_code != 0 { throw out.stderr; }
        parse_json(out.stdout).items
    "#;
    let fixture = format!("{}/tests/fixtures/echo_json.sh", env!("CARGO_MANIFEST_DIR"));
    let cmd = Cmd::new(fixture).with_script("items", script);
    let table = Table::<Cmd, EmptyEntity>::new("items", cmd.clone())
        .with_id_column("name")
        .with_column_of::<i64>("size");
    cmd.vista_factory().from_table(table).unwrap()
}

#[tokio::test]
async fn cmd_contract_advertised() {
    contract::check_advertised(&items()).await;
}

#[tokio::test]
async fn cmd_contract_clone() {
    contract::check_clone(&items(), "name", CborValue::Text("beta".into())).await;
}
