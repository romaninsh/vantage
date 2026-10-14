#![cfg(feature = "mocks")]

//! Columns and the metadata a table derives from them: declaration, flags,
//! lowering from a spec, search and row fields, computed columns, and the
//! Vista metadata built from all of it.

use std::sync::Arc;

use indexmap::IndexMap;
use serde_json::{Value, json};
use vantage_core::Result;
use vantage_dataset::prelude::*;
use vantage_table::column::core::Column;
use vantage_table::mocks::MockColumn;
use vantage_table::prelude::*;
use vantage_table::table::{Orderable, VistaMetadataOptions, retain_matching};
use vantage_types::{EmptyEntity, Record};
use vantage_vista::{ColumnFlag as VistaFlag, ColumnSpec, NoExtras};

type MockTable = Table<MockTableSource, EmptyEntity>;

fn table() -> MockTable {
    MockTable::new("t", MockTableSource::new())
}

fn names<V>(columns: &IndexMap<String, V>) -> Vec<&str> {
    columns.keys().map(String::as_str).collect()
}

// --- Column --------------------------------------------------------------------

#[test]
fn column_starts_with_a_name_and_no_alias_and_no_flags() {
    let column = Column::<String>::new("email");
    assert_eq!(column.name(), "email");
    assert_eq!(column.alias(), None);
    assert!(column.flags().is_empty());
}

#[test]
fn column_alias_and_flags_are_set_by_the_builders() {
    let column = Column::<String>::new("email")
        .with_alias("user_email")
        .with_flags(&[ColumnFlag::Mandatory, ColumnFlag::Hidden])
        .with_flag(ColumnFlag::Searchable);
    assert_eq!(column.alias(), Some("user_email"));
    assert_eq!(column.flags().len(), 3);
    assert!(column.flags().contains(&ColumnFlag::Searchable));
}

#[test]
fn column_from_a_str_is_a_json_column_of_that_name() {
    let column: Column<Value> = "age".into();
    assert_eq!(column.name(), "age");
}

#[test]
fn column_converted_between_types_keeps_name_alias_flags_and_original_type() {
    let typed = Column::<i64>::new("age")
        .with_alias("years")
        .with_flag(ColumnFlag::Indexed);
    let original = ColumnLike::<i64>::get_type(&typed);
    let erased: Column<Value> = Column::from_column(typed);
    assert_eq!(erased.name(), "age");
    assert_eq!(erased.alias(), Some("years"));
    assert!(erased.flags().contains(&ColumnFlag::Indexed));
    assert_eq!(ColumnLike::<Value>::get_type(&erased), original);
    assert_eq!(original, std::any::type_name::<i64>());
}

#[test]
fn column_expression_is_its_name() {
    use vantage_expressions::Expressive;
    let expr = Column::<Value>::new("price").expr();
    assert_eq!(expr.preview(), "price");
}

#[test]
fn column_like_defaults_to_no_alias_and_the_type_parameter_name() {
    let column = MockColumn::<String>::new("name");
    assert_eq!(ColumnLike::<String>::alias(&column), None);
    assert_eq!(
        ColumnLike::<String>::get_type(&column),
        std::any::type_name::<String>()
    );
}

// --- flags ---------------------------------------------------------------------

#[test]
fn every_column_flag_maps_to_its_vista_flag() {
    let expected = [
        (ColumnFlag::Mandatory, VistaFlag::Mandatory),
        (ColumnFlag::Hidden, VistaFlag::Hidden),
        (ColumnFlag::IdField, VistaFlag::Id),
        (ColumnFlag::TitleField, VistaFlag::Title),
        (ColumnFlag::Searchable, VistaFlag::Searchable),
        (ColumnFlag::Indexed, VistaFlag::Other("indexed".into())),
        (ColumnFlag::Label, VistaFlag::Other("label".into())),
        (ColumnFlag::RequestParam, VistaFlag::RequestParam),
    ];
    for (flag, vista) in expected {
        assert_eq!(flag.to_vista(), vista, "{flag:?}");
    }
}

// --- ColumnCollectionExt ------------------------------------------------------

fn flagged_columns() -> Arc<IndexMap<String, Arc<dyn ColumnLike>>> {
    let mut columns: IndexMap<String, Arc<dyn ColumnLike>> = IndexMap::new();
    columns.insert(
        "id".into(),
        Arc::new(Column::<Value>::new("id").with_flags(&[ColumnFlag::Mandatory])),
    );
    columns.insert(
        "name".into(),
        Arc::new(
            Column::<Value>::new("name").with_flags(&[ColumnFlag::Mandatory, ColumnFlag::Hidden]),
        ),
    );
    columns.insert("email".into(), Arc::new(Column::<Value>::new("email")));
    Arc::new(columns)
}

#[test]
fn only_keeps_the_columns_with_the_flag() {
    let mandatory = flagged_columns().only(ColumnFlag::Mandatory);
    assert_eq!(
        mandatory.keys().map(String::as_str).collect::<Vec<_>>(),
        ["id", "name"]
    );
}

#[test]
fn exclude_drops_the_columns_with_the_flag() {
    let visible = flagged_columns().exclude(ColumnFlag::Hidden);
    assert_eq!(
        visible.keys().map(String::as_str).collect::<Vec<_>>(),
        ["id", "email"]
    );
}

#[test]
fn filters_chain() {
    let visible_mandatory = flagged_columns()
        .exclude(ColumnFlag::Hidden)
        .only(ColumnFlag::Mandatory);
    assert_eq!(
        visible_mandatory
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        ["id"]
    );
}

// --- declaring columns --------------------------------------------------------

#[test]
fn columns_are_listed_in_declaration_order_by_name() {
    let table = table()
        .with_column_of::<String>("name")
        .with_column(MockColumn::<i64>::new("age"));
    let mut table = table;
    table.add_column_of::<bool>("active");
    table.add_column(MockColumn::<String>::new("email"));
    assert_eq!(names(table.columns()), ["name", "age", "active", "email"]);
}

#[test]
#[should_panic(expected = "Duplicate column")]
fn declaring_a_column_twice_panics() {
    let _ = table()
        .with_column_of::<String>("name")
        .with_column_of::<String>("name");
}

#[test]
fn a_declared_column_can_be_read_back_by_name_and_a_missing_one_is_none() {
    let table = table().with_column_of::<String>("name");
    assert_eq!(table.get_column::<String>("name").unwrap().name(), "name");
    assert!(table.get_column::<String>("missing").is_none());
}

#[test]
fn copy_columns_from_copies_all_skipping_names_already_present() {
    let source = table()
        .with_column_of::<String>("name")
        .with_column_of::<String>("email");
    let mut target =
        table().with_column(MockColumn::<String>::new("name").with_flag(ColumnFlag::Hidden));
    target.copy_columns_from(&source, None);
    assert_eq!(names(target.columns()), ["name", "email"]);
    assert!(
        target.columns()["name"]
            .flags()
            .contains(&ColumnFlag::Hidden),
        "an existing column is kept as it was"
    );
}

#[test]
fn copy_columns_from_copies_only_the_listed_names() {
    let source = table()
        .with_column_of::<String>("name")
        .with_column_of::<String>("email")
        .with_column_of::<String>("note");
    let mut target = table();
    target.copy_columns_from(&source, Some(&["note", "name"]));
    assert_eq!(names(target.columns()), ["name", "note"]);
}

// --- id and titles ------------------------------------------------------------

#[test]
fn id_field_is_the_column_added_by_with_id_column() {
    let table = table().with_id_column("code");
    assert_eq!(table.id_field().unwrap().name(), "code");
    assert_eq!(table.id_field_name(), "code");
    assert!(table.columns().contains_key("code"));
}

#[test]
fn id_field_name_is_id_when_none_is_set() {
    let table = table();
    assert!(table.id_field().is_none());
    assert_eq!(table.id_field_name(), "id");
}

#[test]
fn set_id_field_flags_an_already_added_column() {
    let mut table = table().with_column_of::<String>("uuid");
    table.set_id_field("uuid");
    assert_eq!(table.id_field_name(), "uuid");
}

#[test]
fn title_columns_keep_call_order_and_the_first_is_the_title_field() {
    let table = table()
        .with_title_column_of::<String>("name")
        .with_title_column(MockColumn::<String>::new("nick"));
    assert_eq!(table.title_fields(), ["name", "nick"]);
    assert_eq!(table.title_field().unwrap().name(), "name");
    assert_eq!(names(table.columns()), ["name", "nick"]);
}

#[test]
fn add_title_field_marks_an_existing_column_once() {
    let mut table = table().with_column_of::<String>("name");
    table.add_title_field("name");
    table.add_title_field("name");
    assert_eq!(table.title_fields(), ["name"]);
}

#[test]
fn id_text_and_auto_id_default_to_off_and_are_set_by_their_builders() {
    let plain = table();
    assert!(!plain.id_is_text());
    assert!(!plain.has_auto_id());
    let flagged = table().with_text_id().with_auto_id();
    assert!(flagged.id_is_text());
    assert!(flagged.has_auto_id());
}

#[test]
fn table_name_and_source_name_the_table_and_can_be_overridden() {
    let mut table = table();
    assert_eq!(table.table_name(), "t");
    assert_eq!(table.source(), "t");
    table.set_table_name("renamed");
    assert_eq!(table.table_name(), "renamed");
}

#[test]
fn with_runs_the_callback_on_the_table_and_returns_it() {
    let table = table().with(|t| {
        t.add_column_of::<String>("name");
    });
    assert_eq!(names(table.columns()), ["name"]);
}

// --- search columns and row fields ---------------------------------------------

fn search_names(table: &MockTable) -> Vec<String> {
    table
        .search_columns()
        .into_iter()
        .map(|c| c.name().to_string())
        .collect()
}

#[test]
fn search_columns_are_every_column_when_none_is_flagged_searchable() {
    let table = table()
        .with_column_of::<String>("name")
        .with_column_of::<String>("note");
    assert_eq!(search_names(&table), ["name", "note"]);
}

#[test]
fn search_columns_are_only_the_flagged_ones_when_any_is() {
    let table = table()
        .with_column_of::<String>("name")
        .with_column(MockColumn::<String>::new("title").with_flag(ColumnFlag::Searchable));
    assert_eq!(search_names(&table), ["title"]);
}

#[test]
fn row_fields_leave_out_request_params_but_keep_hidden_columns() {
    let table = table()
        .with_column(MockColumn::<String>::new("region").with_flag(ColumnFlag::RequestParam))
        .with_column_of::<String>("name")
        .with_column(MockColumn::<String>::new("owner").with_flag(ColumnFlag::Hidden));
    assert_eq!(table.row_fields(), ["name", "owner"]);
}

fn numbered_rows() -> IndexMap<String, Record<i64>> {
    let row = |pairs: &[(&str, i64)]| -> Record<i64> {
        pairs.iter().map(|(k, v)| (k.to_string(), *v)).collect()
    };
    [
        ("a".to_string(), row(&[("size", 1)])),
        ("b".to_string(), row(&[("size", 2)])),
        ("c".to_string(), row(&[])),
    ]
    .into_iter()
    .collect()
}

#[test]
fn retain_matching_keeps_rows_equal_on_every_filter() {
    let mut rows = numbered_rows();
    let row_fields = vec!["size".to_string()];
    retain_matching(&mut rows, &[("size", &2)], &row_fields, |a, b| a == b);
    assert_eq!(rows.keys().collect::<Vec<_>>(), ["b"]);
}

#[test]
fn retain_matching_fails_a_row_missing_a_field_that_rows_carry() {
    let mut rows = numbered_rows();
    let row_fields = vec!["size".to_string()];
    retain_matching(&mut rows, &[("size", &1)], &row_fields, |a, b| a == b);
    assert_eq!(rows.keys().collect::<Vec<_>>(), ["a"]);
}

#[test]
fn retain_matching_passes_a_row_missing_a_field_the_backend_applied() {
    let mut rows = numbered_rows();
    let row_fields = vec!["size".to_string()];
    retain_matching(&mut rows, &[("prefix", &9)], &row_fields, |a, b| a == b);
    assert_eq!(rows.len(), 3);
}

// --- Column::from_spec --------------------------------------------------------

fn string_column(name: &str, ty: &str) -> Result<Column<String>> {
    assert_eq!(ty, "string");
    Ok(Column::new(name))
}

#[test]
fn from_spec_asks_for_the_string_type_when_the_spec_names_none() {
    let spec = ColumnSpec::<NoExtras>::new();
    let column = Column::from_spec("name", &spec, None, string_column).unwrap();
    assert_eq!(column.name(), "name");
}

#[test]
fn from_spec_passes_the_specs_type_to_the_constructor() {
    let mut spec = ColumnSpec::<NoExtras>::new();
    spec.col_type = Some("int".into());
    let column = Column::from_spec("age", &spec, None, |name, ty| {
        assert_eq!(ty, "int");
        Ok(Column::<String>::new(name))
    })
    .unwrap();
    assert_eq!(column.name(), "age");
}

#[test]
fn from_spec_applies_the_alias_unless_it_repeats_the_name() {
    let spec = ColumnSpec::<NoExtras>::new();
    let aliased = Column::from_spec("name", &spec, Some("db_name".into()), string_column).unwrap();
    assert_eq!(aliased.alias(), Some("db_name"));
    let same = Column::from_spec("name", &spec, Some("name".into()), string_column).unwrap();
    assert_eq!(same.alias(), None);
}

#[test]
fn from_spec_carries_over_every_flag_with_a_typed_counterpart() {
    let spec = ColumnSpec::<NoExtras>::new()
        .with_flag(VistaFlag::Mandatory)
        .with_flag(VistaFlag::Hidden)
        .with_flag(VistaFlag::Searchable)
        .with_flag("indexed")
        .with_flag("label")
        .with_flag(VistaFlag::RequestParam);
    let column = Column::from_spec("status", &spec, None, string_column).unwrap();
    for flag in [
        ColumnFlag::Mandatory,
        ColumnFlag::Hidden,
        ColumnFlag::Searchable,
        ColumnFlag::Indexed,
        ColumnFlag::Label,
        ColumnFlag::RequestParam,
    ] {
        assert!(column.flags().contains(&flag), "{flag:?}");
    }
}

#[test]
fn from_spec_leaves_the_id_and_title_designations_to_the_table() {
    let spec = ColumnSpec::<NoExtras>::new()
        .with_flag(VistaFlag::Id)
        .with_flag(VistaFlag::Title);
    let column = Column::from_spec("code", &spec, None, string_column).unwrap();
    assert!(column.flags().is_empty());
}

#[test]
fn from_spec_returns_the_constructors_error() {
    let spec = ColumnSpec::<NoExtras>::new();
    let result = Column::<String>::from_spec("x", &spec, None, |_, _| {
        Err(vantage_core::error!("unknown type"))
    });
    assert!(result.is_err());
}

// --- computed columns ----------------------------------------------------------

fn computed(table: &MockTable) -> Vec<(usize, String)> {
    table
        .computed_columns()
        .map(|(at, c)| (at, c.name.clone()))
        .collect()
}

#[test]
fn a_computed_column_is_positioned_after_every_column_added_so_far() {
    let mut table = table()
        .with_column_of::<String>("a")
        .with_column_of::<String>("b");
    table.add_computed_column(vantage_vista::Column::new("x", "string"));
    table.add_column_of::<String>("c");
    assert_eq!(computed(&table), [(2, "x".to_string())]);
}

#[test]
fn a_computed_column_replaces_a_stored_one_of_the_same_name() {
    let mut table = table()
        .with_column_of::<String>("a")
        .with_column_of::<String>("b");
    table.add_computed_column(vantage_vista::Column::new("x", "string"));
    table.add_column_of::<String>("c");

    table.add_computed_column(vantage_vista::Column::new("a", "string"));

    assert_eq!(names(table.columns()), ["b", "c"]);
    assert_eq!(
        computed(&table),
        [(1, "x".to_string()), (3, "a".to_string())]
    );
}

#[test]
fn a_lazy_expression_registers_its_name_as_a_column_and_counts_as_calculated() {
    let table = table().with_lazy_expression("greeting", |_| async { Ok(json!("hi")) });
    assert!(table.columns().contains_key("greeting"));
    assert!(table.is_calculated_column("greeting"));
    assert!(!table.is_calculated_column("other"));
}

#[test]
fn an_expression_column_counts_as_calculated_but_a_stored_one_does_not() {
    let table = table()
        .with_column_of::<String>("name")
        .with_expression("shout", |_| vantage_expressions::expr_any!("upper(name)"));
    assert!(table.is_calculated_column("shout"));
    assert!(!table.is_calculated_column("name"));
}

#[test]
fn an_imported_column_is_unknown_to_a_table_that_imported_nothing() {
    assert!(!table().is_imported_column("client.name"));
}

#[tokio::test]
async fn lazy_expressions_apply_in_declaration_order_each_seeing_the_earlier_columns() {
    let src = MockTableSource::new()
        .with_data("t", vec![json!({"id": "1", "name": "Alice"})])
        .await;
    let table = MockTable::new("t", src)
        .with_lazy_expression("greeting", |r| {
            let name = r["name"].as_str().unwrap_or_default().to_string();
            async move { Ok(json!(format!("hello {name}"))) }
        })
        .with_lazy_expression("shout", |r| {
            let greeting = r["greeting"].as_str().unwrap_or_default().to_string();
            async move { Ok(json!(greeting.to_uppercase())) }
        });

    let rows = table.list_values().await.unwrap();
    assert_eq!(rows["1"]["greeting"], json!("hello Alice"));
    assert_eq!(rows["1"]["shout"], json!("HELLO ALICE"));
    let row = table.get_value("1").await.unwrap().unwrap();
    assert_eq!(row["shout"], json!("HELLO ALICE"));
    let (_, some) = table.get_some_value().await.unwrap().unwrap();
    assert_eq!(some["shout"], json!("HELLO ALICE"));
}

#[tokio::test]
async fn streamed_values_carry_the_lazy_columns_too() {
    let src = MockTableSource::new()
        .with_data("t", vec![json!({"id": "1", "name": "Alice"})])
        .await;
    let table = MockTable::new("t", src).with_lazy_expression("tag", |_| async { Ok(json!("x")) });

    let mut stream = table.stream_values();
    let mut streamed = Vec::new();
    while let Some(item) = std::future::poll_fn(|cx| stream.as_mut().poll_next(cx)).await {
        streamed.push(item.unwrap());
    }
    assert_eq!(streamed.len(), 1);
    assert_eq!(streamed[0].0, "1");
    assert_eq!(streamed[0].1["tag"], json!("x"));
}

#[tokio::test]
async fn streamed_values_without_lazy_columns_are_the_listed_rows() {
    let src = MockTableSource::new()
        .with_data(
            "t",
            vec![
                json!({"id": "1", "name": "a"}),
                json!({"id": "2", "name": "b"}),
            ],
        )
        .await;
    let table = MockTable::new("t", src);
    let mut stream = table.stream_values();
    let mut ids = Vec::new();
    while let Some(item) = std::future::poll_fn(|cx| stream.as_mut().poll_next(cx)).await {
        ids.push(item.unwrap().0);
    }
    assert_eq!(ids, ["1", "2"]);
}

#[tokio::test]
async fn a_failing_lazy_expression_fails_the_read() {
    let src = MockTableSource::new()
        .with_data("t", vec![json!({"id": "1"})])
        .await;
    let table = MockTable::new("t", src)
        .with_lazy_expression("bad", |_| async { Err(vantage_core::error!("no value")) });
    assert!(table.list_values().await.is_err());
    assert!(table.get_value("1").await.is_err());
}

#[tokio::test]
async fn apply_lazy_expressions_fills_a_record_the_caller_fetched() {
    let table = table().with_lazy_expression("tag", |r| {
        let n = r["n"].as_i64().unwrap_or(0);
        async move { Ok(json!(n * 2)) }
    });
    let mut record = Record::from(json!({"n": 21}));
    table.apply_lazy_expressions(&mut record).await.unwrap();
    assert_eq!(record["tag"], json!(42));
}

// --- Vista metadata -----------------------------------------------------------

fn users() -> MockTable {
    let mut table = MockTable::new("users", MockTableSource::new())
        .with_id_column("id")
        .with_title_column_of::<String>("name");
    table.add_computed_column(vantage_vista::Column::new("shout", "string"));
    table
}

fn flags_of(metadata: &vantage_vista::VistaMetadata, column: &str) -> Vec<VistaFlag> {
    metadata.columns[column].flags().to_vec()
}

#[test]
fn vista_metadata_names_the_id_and_flags_titles_by_default() {
    let metadata = users().vista_metadata(VistaMetadataOptions::default());
    assert_eq!(metadata.id_column.as_deref(), Some("id"));
    assert!(flags_of(&metadata, "id").is_empty());
    assert_eq!(flags_of(&metadata, "name"), [VistaFlag::Title]);
    assert_eq!(
        metadata
            .columns
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        ["id", "name", "shout"]
    );
}

#[test]
fn vista_metadata_without_an_id_field_names_none() {
    let metadata = table()
        .with_column_of::<String>("name")
        .vista_metadata(VistaMetadataOptions::default());
    assert_eq!(metadata.id_column, None);
}

#[test]
fn vista_metadata_flags_every_stored_column_orderable_on_request() {
    let metadata = users().vista_metadata(VistaMetadataOptions {
        orderable: Orderable::All,
        ..VistaMetadataOptions::default()
    });
    assert!(metadata.columns["id"].has_flag(VistaFlag::Orderable));
    assert!(metadata.columns["name"].has_flag(VistaFlag::Orderable));
    assert!(!metadata.columns["shout"].has_flag(VistaFlag::Orderable));
}

#[test]
fn vista_metadata_flags_stored_columns_orderable_except_imported_ones() {
    let metadata = users().vista_metadata(VistaMetadataOptions {
        orderable: Orderable::ExceptImported,
        ..VistaMetadataOptions::default()
    });
    assert!(metadata.columns["name"].has_flag(VistaFlag::Orderable));
}

#[test]
fn vista_metadata_flags_the_id_and_searchable_titles_on_request() {
    let metadata = users().vista_metadata(VistaMetadataOptions {
        id_flag: true,
        searchable_titles: true,
        ..VistaMetadataOptions::default()
    });
    assert!(metadata.columns["id"].has_flag(VistaFlag::Id));
    assert!(metadata.columns["name"].has_flag(VistaFlag::Title));
    assert!(metadata.columns["name"].has_flag(VistaFlag::Searchable));
}

#[test]
fn vista_metadata_flags_an_auto_id_column_auto() {
    let metadata = users()
        .with_auto_id()
        .vista_metadata(VistaMetadataOptions::default());
    assert!(metadata.columns["id"].has_flag(VistaFlag::Auto));
}

#[test]
fn vista_metadata_flags_computed_columns_calculated_on_request() {
    let table = table()
        .with_column_of::<String>("name")
        .with_lazy_expression("greeting", |_| async { Ok(json!("hi")) });
    let off = table.vista_metadata(VistaMetadataOptions::default());
    assert!(!off.columns["greeting"].has_flag(VistaFlag::Calculated));

    let on = table.vista_metadata(VistaMetadataOptions {
        calculated: true,
        ..VistaMetadataOptions::default()
    });
    assert!(!on.columns["name"].has_flag(VistaFlag::Calculated));
    assert!(on.columns["greeting"].has_flag(VistaFlag::Calculated));
}

fn flagged_table() -> MockTable {
    [
        ColumnFlag::Mandatory,
        ColumnFlag::Hidden,
        ColumnFlag::IdField,
        ColumnFlag::TitleField,
        ColumnFlag::Searchable,
        ColumnFlag::Indexed,
        ColumnFlag::Label,
        ColumnFlag::RequestParam,
    ]
    .into_iter()
    .fold(table(), |table, flag| {
        table.with_column(MockColumn::<String>::new(format!("{flag:?}")).with_flag(flag))
    })
}

#[test]
fn vista_metadata_carries_every_column_flag_when_asked() {
    let metadata = flagged_table().vista_metadata(VistaMetadataOptions {
        column_flags: true,
        ..VistaMetadataOptions::default()
    });
    let expected = [
        ("Mandatory", VistaFlag::Mandatory),
        ("Hidden", VistaFlag::Hidden),
        ("IdField", VistaFlag::Id),
        ("TitleField", VistaFlag::Title),
        ("Searchable", VistaFlag::Searchable),
        ("Indexed", VistaFlag::Other("indexed".into())),
        ("Label", VistaFlag::Other("label".into())),
        ("RequestParam", VistaFlag::RequestParam),
    ];
    for (column, flag) in expected {
        assert_eq!(flags_of(&metadata, column), [flag], "{column}");
    }
}

#[test]
fn vista_metadata_always_carries_hidden_label_and_request_param_only() {
    let metadata = flagged_table().vista_metadata(VistaMetadataOptions::default());
    assert_eq!(flags_of(&metadata, "Hidden"), [VistaFlag::Hidden]);
    assert_eq!(
        flags_of(&metadata, "Label"),
        [VistaFlag::Other("label".into())]
    );
    assert_eq!(
        flags_of(&metadata, "RequestParam"),
        [VistaFlag::RequestParam]
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
}

#[test]
fn a_column_with_several_presentation_flags_carries_all_of_them() {
    let table = table().with_column(
        MockColumn::<String>::new("tag")
            .with_flag(ColumnFlag::RequestParam)
            .with_flag(ColumnFlag::Label)
            .with_flag(ColumnFlag::Hidden),
    );
    let flags = flags_of(
        &table.vista_metadata(VistaMetadataOptions::default()),
        "tag",
    );
    assert_eq!(flags.len(), 3);
    for flag in [
        VistaFlag::Hidden,
        VistaFlag::Other("label".into()),
        VistaFlag::RequestParam,
    ] {
        assert!(flags.contains(&flag), "{flag:?}");
    }
}

#[test]
fn vista_columns_carry_name_and_the_presentation_flags_only() {
    let columns = flagged_table().vista_columns();
    let flags = |name: &str| {
        columns
            .iter()
            .find(|c| c.name == name)
            .unwrap()
            .flags()
            .to_vec()
    };
    assert_eq!(columns.len(), 8);
    assert_eq!(flags("Hidden"), [VistaFlag::Hidden]);
    assert_eq!(flags("Label"), [VistaFlag::Other("label".into())]);
    assert_eq!(flags("RequestParam"), [VistaFlag::RequestParam]);
    assert!(flags("Indexed").is_empty());
}

#[test]
fn vista_metadata_without_references_or_contained_relations_has_none() {
    let metadata = users().vista_metadata(VistaMetadataOptions {
        references: true,
        contained: true,
        ..VistaMetadataOptions::default()
    });
    assert!(metadata.references.is_empty());
    assert!(users().vista_references().is_empty());
    assert!(users().vista_contained().is_empty());
    assert!(users().contained_relation("lines").is_none());
}
