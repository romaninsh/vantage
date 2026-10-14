//! A set narrowed on a dotted path (`address.city`): writes are judged by
//! the nested value, never by a literal dotted key.

use ciborium::Value as CborValue;
use vantage_core::Result;
use vantage_dataset::traits::WritableValueSet;
use vantage_types::Record;
use vantage_vista::{Column, FilterOp, Vista, VistaMetadata, mocks::MockShell};

use crate::support::record;
use crate::warmed_dio;

fn address(city: &str) -> CborValue {
    CborValue::Map(vec![(
        CborValue::Text("city".into()),
        CborValue::Text(city.into()),
    )])
}

/// People with a nested `address` map; person 1 lives in Riga. Returns the
/// facade narrowed to `address.city == "Riga"` and the master's shell.
async fn riga_people() -> Result<(Vista, MockShell)> {
    let shell = MockShell::new().with_metadata(
        VistaMetadata::new()
            .with_column(Column::new("id", "String").with_flag("id"))
            .with_column(Column::new("name", "String"))
            .with_column(Column::new("address", "Map"))
            .with_id_column("id"),
    );
    let mut row = record(&[("id", "1"), ("name", "Ada")]);
    row.insert("address".into(), address("Riga"));
    shell.set_record("1", row);
    let dio = warmed_dio(Vista::new("people", shell.clone())).await?;
    let mut riga = dio.cached();
    riga.add_condition("address.city", FilterOp::Eq, "Riga")?;
    Ok((riga, shell))
}

/// A set narrowed on a dotted path (`address.city`) has no flat cell to fill:
/// a write must hold the value at the path itself, and nothing writes the
/// literal key `"address.city"`.
#[tokio::test]
async fn dotted_path_narrowing_fills_no_literal_key() -> Result<()> {
    let (riga, shell) = riga_people().await?;

    let no_address = riga.insert_value("2", &record(&[("id", "2")])).await;
    assert!(no_address.unwrap_err().is_conflict(), "nothing to fill");
    let mut elsewhere = record(&[("id", "3")]);
    elsewhere.insert("address".into(), address("Oslo"));
    assert!(
        riga.insert_value("3", &elsewhere)
            .await
            .unwrap_err()
            .is_conflict()
    );
    let mut moved = Record::new();
    moved.insert("address".into(), address("Oslo"));
    assert!(
        riga.patch_value("1", &moved)
            .await
            .unwrap_err()
            .is_conflict()
    );

    let mut inside = record(&[("id", "4")]);
    inside.insert("address".into(), address("Riga"));
    riga.insert_value("4", &inside).await?;
    let stored = shell.get_record("4").expect("row 4 written");
    assert!(stored.get("address.city").is_none(), "{stored:?}");
    assert_eq!(stored.get("address"), Some(&address("Riga")));
    Ok(())
}

/// A literal `"address.city"` key in a written record can't stand in for the
/// nested value: the set check walks the path.
#[tokio::test]
async fn dotted_path_set_ignores_a_literal_dotted_key() -> Result<()> {
    let (riga, _shell) = riga_people().await?;
    let mut disguised = record(&[("id", "5"), ("address.city", "Riga")]);
    disguised.insert("address".into(), address("Oslo"));
    assert!(
        riga.insert_value("5", &disguised)
            .await
            .unwrap_err()
            .is_conflict()
    );
    let mut disguised_patch = record(&[("address.city", "Riga")]);
    disguised_patch.insert("address".into(), address("Oslo"));
    assert!(
        riga.patch_value("1", &disguised_patch)
            .await
            .unwrap_err()
            .is_conflict()
    );
    Ok(())
}

/// A patch that keeps the row in the dotted set goes through.
#[tokio::test]
async fn dotted_path_set_takes_an_in_set_patch() -> Result<()> {
    let (riga, shell) = riga_people().await?;
    riga.patch_value("1", &record(&[("name", "Ada L.")])).await?;
    let stored = shell.get_record("1").expect("row 1");
    assert_eq!(stored.get("name"), Some(&CborValue::Text("Ada L.".into())));
    assert_eq!(stored.get("address"), Some(&address("Riga")));
    Ok(())
}
