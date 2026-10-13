use super::*;
use crate::mocks::{MockReference, mock_table_source::MockTableSource};
use crate::references::Cardinality;

#[test]
fn computed_column_replacing_a_stored_one_keeps_positions() {
    let mut table = Table::<MockTableSource, EmptyEntity>::new("t", MockTableSource::new())
        .with_column_of::<String>("a")
        .with_column_of::<String>("b");
    table.add_computed_column(vantage_vista::Column::new("x", "string"));
    table.add_column_of::<String>("c");
    assert_eq!(
        table
            .computed_columns()
            .map(|(at, c)| (at, c.name.as_str()))
            .collect::<Vec<_>>(),
        vec![(2, "x")]
    );

    table.add_computed_column(vantage_vista::Column::new("a", "string"));

    let stored: Vec<&str> = table.columns().keys().map(String::as_str).collect();
    assert_eq!(stored, vec!["b", "c"]);
    assert_eq!(
        table
            .computed_columns()
            .map(|(at, c)| (at, c.name.as_str()))
            .collect::<Vec<_>>(),
        vec![(1, "x"), (3, "a")]
    );
}

#[test]
fn vista_references_build_each_join_by_its_kind() {
    use vantage_vista::{JoinKey, Reference as VistaReference};

    let mut table = Table::<MockTableSource, EmptyEntity>::new("t", MockTableSource::new())
        .with_id_column("code");
    let owner = MockReference::new(Cardinality::HasOne, "owner_id");
    let lines = MockReference::new(Cardinality::HasMany, "t_code");
    table.add_ref("owner", Box::new(owner));
    table.add_ref("lines", Box::new(lines));

    assert_eq!(
        table.vista_references(),
        [
            VistaReference::has_one("owner", "target_table", JoinKey::to_target_id("owner_id")),
            VistaReference::has_many("lines", "target_table", JoinKey::new("code", "t_code")),
        ]
    );
}

#[test]
fn a_cardinality_is_its_join_kind() {
    use vantage_vista::ReferenceKind;
    assert_eq!(
        ReferenceKind::from(Cardinality::HasOne),
        ReferenceKind::HasOne
    );
    assert_eq!(
        ReferenceKind::from(Cardinality::HasMany),
        ReferenceKind::HasMany
    );
}
