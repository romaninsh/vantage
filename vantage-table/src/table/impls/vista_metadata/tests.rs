use super::*;
use crate::mocks::{mock_column::MockColumn, mock_table_source::MockTableSource};
use vantage_types::EmptyEntity;

fn table() -> Table<MockTableSource, EmptyEntity> {
    let mut table = Table::new("users", MockTableSource::new())
        .with_id_column("id")
        .with_title_column_of::<String>("name");
    table.add_computed_column(VistaColumn::new("shout", "string"));
    table
}

fn flags_of(metadata: &VistaMetadata, column: &str) -> Vec<ColumnFlag> {
    metadata.columns[column].flags().to_vec()
}

/// Every typed flag, each on its own column named after it.
fn flagged_table() -> Table<MockTableSource, EmptyEntity> {
    let every = [
        TableColumnFlag::Mandatory,
        TableColumnFlag::Hidden,
        TableColumnFlag::IdField,
        TableColumnFlag::TitleField,
        TableColumnFlag::Searchable,
        TableColumnFlag::Indexed,
        TableColumnFlag::Label,
        TableColumnFlag::RequestParam,
    ];
    every.into_iter().fold(
        Table::new("flags", MockTableSource::new()),
        |table, flag| {
            table.with_column(MockColumn::<String>::new(format!("{flag:?}")).with_flag(flag))
        },
    )
}

#[test]
fn defaults_mark_only_the_id_and_titles() {
    let metadata = table().vista_metadata(VistaMetadataOptions::default());
    assert_eq!(metadata.id_column.as_deref(), Some("id"));
    assert!(flags_of(&metadata, "id").is_empty());
    assert_eq!(flags_of(&metadata, "name"), [ColumnFlag::Title]);
    let names: Vec<&String> = metadata.columns.keys().collect();
    assert_eq!(names, ["id", "name", "shout"]);
}

#[test]
fn options_add_orderable_id_and_searchable_title_flags() {
    let metadata = table().vista_metadata(VistaMetadataOptions {
        orderable: Orderable::All,
        id_flag: true,
        searchable_titles: true,
        ..VistaMetadataOptions::default()
    });
    assert_eq!(
        flags_of(&metadata, "id"),
        [ColumnFlag::Orderable, ColumnFlag::Id]
    );
    assert_eq!(
        flags_of(&metadata, "name"),
        [
            ColumnFlag::Orderable,
            ColumnFlag::Title,
            ColumnFlag::Searchable
        ]
    );
    assert!(flags_of(&metadata, "shout").is_empty());
}

#[test]
fn every_typed_flag_maps_with_column_flags() {
    let metadata = flagged_table().vista_metadata(VistaMetadataOptions {
        column_flags: true,
        ..VistaMetadataOptions::default()
    });
    let expected = [
        ("Mandatory", ColumnFlag::Mandatory),
        ("Hidden", ColumnFlag::Hidden),
        ("IdField", ColumnFlag::Id),
        ("TitleField", ColumnFlag::Title),
        ("Searchable", ColumnFlag::Searchable),
        ("Indexed", ColumnFlag::Other("indexed".into())),
        ("Label", ColumnFlag::Other("label".into())),
        ("RequestParam", ColumnFlag::RequestParam),
    ];
    assert_eq!(metadata.columns.len(), expected.len());
    for (column, flag) in expected {
        assert_eq!(flags_of(&metadata, column), [flag], "{column}");
    }
}

#[test]
fn presentation_flags_map_without_column_flags() {
    let metadata = flagged_table().vista_metadata(VistaMetadataOptions::default());
    assert_eq!(flags_of(&metadata, "Hidden"), [ColumnFlag::Hidden]);
    assert_eq!(
        flags_of(&metadata, "Label"),
        [ColumnFlag::Other("label".into())]
    );
    assert_eq!(
        flags_of(&metadata, "RequestParam"),
        [ColumnFlag::RequestParam]
    );
    for column in [
        "Mandatory",
        "IdField",
        "TitleField",
        "Searchable",
        "Indexed",
    ] {
        assert!(flags_of(&metadata, column).is_empty(), "{column}");
    }
    let contained = flagged_table().vista_columns();
    let flags_in_contained = |name: &str| {
        let column = contained.iter().find(|c| c.name == name).unwrap();
        column.flags().to_vec()
    };
    assert_eq!(flags_in_contained("Hidden"), [ColumnFlag::Hidden]);
    assert_eq!(
        flags_in_contained("Label"),
        [ColumnFlag::Other("label".into())]
    );
    assert_eq!(
        flags_in_contained("RequestParam"),
        [ColumnFlag::RequestParam]
    );
    assert!(flags_in_contained("Indexed").is_empty());
}

#[test]
fn a_column_with_several_flags_lists_them_by_name() {
    let table = Table::<MockTableSource, EmptyEntity>::new("t", MockTableSource::new())
        .with_column(
            MockColumn::<String>::new("tag")
                .with_flag(TableColumnFlag::RequestParam)
                .with_flag(TableColumnFlag::Label)
                .with_flag(TableColumnFlag::Hidden),
        );
    let metadata = table.vista_metadata(VistaMetadataOptions::default());
    assert_eq!(
        flags_of(&metadata, "tag"),
        [
            ColumnFlag::Hidden,
            ColumnFlag::Other("label".into()),
            ColumnFlag::RequestParam
        ]
    );
}

#[test]
fn auto_id_flags_the_id_column() {
    let metadata = table()
        .with_auto_id()
        .vista_metadata(VistaMetadataOptions::default());
    assert!(metadata.columns["id"].has_flag(ColumnFlag::Auto));
}
