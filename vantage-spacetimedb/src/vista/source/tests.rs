//! Offline checks of the shell's narrowing, refusals and preview. None of
//! these send a request: the client points at a port nothing listens on.

use ciborium::Value as CborValue;
use spacetimedb_sats::{AlgebraicType, ProductType, ProductTypeElement};
use vantage_types::Record;
use vantage_vista::{
    Capability, Column, Condition, FilterOp, TableShell, Vista, VistaCapabilities, VistaMetadata,
};

use super::SpacetimeTableShell;
use crate::client::SpacetimeDb;
use crate::schema::RowIdentity;

/// `game(id: u64 primary key, status: string, score: i64)`.
fn shell(capabilities: VistaCapabilities) -> SpacetimeTableShell {
    let row_type = ProductType::new(
        [
            ("id", AlgebraicType::U64),
            ("status", AlgebraicType::String),
            ("score", AlgebraicType::I64),
        ]
        .into_iter()
        .map(|(name, ty)| ProductTypeElement::new(ty, Some(name.into())))
        .collect(),
    );
    let metadata = VistaMetadata::new()
        .with_column(Column::new("id", "int"))
        .with_column(Column::new("status", "string"))
        .with_column(Column::new("score", "int"))
        .with_id_column("id");
    SpacetimeTableShell::new(
        SpacetimeDb::new("http://127.0.0.1:9", "test"),
        "game".to_string(),
        metadata,
        RowIdentity::PrimaryKey("id".to_string()),
        row_type,
        capabilities,
    )
}

fn filtering() -> VistaCapabilities {
    [Capability::FilterOps, Capability::Subscribe]
        .into_iter()
        .collect()
}

fn query(shell: &SpacetimeTableShell) -> Option<String> {
    shell.preview().query
}

#[test]
fn comparisons_are_pushed_into_the_where_clause() {
    let mut shell = shell(filtering());
    shell
        .add_condition(&Condition::new("score", FilterOp::Gte, 3))
        .unwrap();
    shell
        .add_condition(&Condition::new("status", FilterOp::Ne, "ended"))
        .unwrap();
    assert_eq!(
        query(&shell).as_deref(),
        Some("SELECT * FROM game WHERE score >= 3 AND status != 'ended'")
    );
}

#[test]
fn operators_the_dialect_cannot_spell_are_refused_and_not_recorded() {
    let mut shell = shell(filtering());
    let before = query(&shell);
    let set = CborValue::Array(vec![CborValue::Text("ended".into())]);
    for condition in [
        Condition::new("status", FilterOp::Like, "end"),
        Condition::new("status", FilterOp::InSet, set.clone()),
        Condition::new("status", FilterOp::NotInSet, set),
    ] {
        let err = shell.add_condition(&condition).unwrap_err();
        assert!(err.is_unsupported(), "{:?}: {err}", condition.op);
    }
    assert_eq!(query(&shell), before, "a refused condition must not narrow");
}

#[test]
fn an_unknown_column_is_a_plain_error() {
    let err = shell(filtering())
        .add_condition(&Condition::new("nope", FilterOp::Eq, 1))
        .unwrap_err();
    assert!(!err.is_unsupported() && !err.is_unimplemented(), "{err}");
    assert!(err.to_string().contains("Unknown column"), "{err}");
}

#[test]
fn a_null_or_array_has_no_literal_and_is_left_to_the_caller() {
    let mut shell = shell(filtering());
    let before = query(&shell);
    for value in [CborValue::Null, CborValue::Array(vec![])] {
        let err = shell
            .add_condition(&Condition::new("status", FilterOp::Eq, value))
            .unwrap_err();
        assert!(err.is_unsupported(), "{err}");
        assert!(err.context.contains_key("reason"), "{err:?}");
    }
    assert_eq!(query(&shell), before, "a refused condition must not narrow");
}

/// Text the dialect can't carry is a fault in the value, not a refusal a
/// caller would evaluate locally.
#[test]
fn an_unencodable_text_operand_is_an_error_not_a_refusal() {
    let err = shell(filtering())
        .add_condition(&Condition::new("status", FilterOp::Eq, "ok\0DROP"))
        .unwrap_err();
    assert!(!err.is_unsupported() && !err.is_unimplemented(), "{err}");
}

#[test]
fn narrowing_a_clone_leaves_the_original_alone() {
    let vista = Vista::new("game", shell(filtering()));
    let narrowed = vista
        .clone()
        .with_condition("status", FilterOp::Eq, "playing")
        .unwrap();
    assert_eq!(vista.preview().query.as_deref(), Some("SELECT * FROM game"));
    assert_eq!(
        narrowed.preview().query.as_deref(),
        Some("SELECT * FROM game WHERE status = 'playing'")
    );
}

#[tokio::test]
async fn a_window_past_the_start_is_refused_without_a_request() {
    let err = shell(filtering()).fetch_window(10, 5).await.unwrap_err();
    assert!(err.is_unsupported(), "{err}");
}

#[tokio::test]
async fn unadvertised_writes_are_refused_without_a_request() {
    let shell = shell(filtering());
    let row = Record::<CborValue>::new();
    assert!(
        shell
            .insert(Some("1"), &row)
            .await
            .unwrap_err()
            .is_unsupported()
    );
    assert!(shell.patch("1", &row).await.unwrap_err().is_unsupported());
    assert!(shell.replace("1", &row).await.unwrap_err().is_unsupported());
    assert!(shell.delete("1").await.unwrap_err().is_unsupported());
    assert!(shell.delete_all().await.unwrap_err().is_unsupported());
}

#[test]
fn traversal_is_refused() {
    let Err(err) = shell(filtering()).get_ref("player", None) else {
        panic!("a SpacetimeDB relation has no references");
    };
    assert!(err.is_unsupported(), "{err}");
    assert!(err.context["capability"].contains("traverse"), "{err}");
}
