use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_dataset::traits::{InsertableValueSet, ReadableValueSet, WritableValueSet};
use vantage_vista::mocks::MockShell;
use vantage_vista::{
    Capability, Column, ColumnFlag, Condition, FilterOp, IdGenerator, NativeCondition,
    SortDirection, Vista, VistaCapabilities, VistaMetadata, VistaQuery,
};

use super::{i, rec, t};

#[derive(Clone)]
struct Wrap {
    inner: Box<dyn vantage_vista::TableShell>,
}

vantage_vista::forward_table_shell!(Wrap, inner, {});

fn metadata() -> VistaMetadata {
    VistaMetadata::new()
        .with_column(
            Column::new("id", "String")
                .with_flag(ColumnFlag::Id)
                .with_flag(ColumnFlag::Orderable),
        )
        .with_column(
            Column::new("name", "String")
                .with_flag(ColumnFlag::Title)
                .with_flag(ColumnFlag::Searchable)
                .with_flag(ColumnFlag::Orderable),
        )
        .with_column(Column::new("age", "i64").with_flag(ColumnFlag::Orderable))
        .with_column(Column::new("note", "String"))
        .with_column(Column::new("dims", "map"))
        .with_id_column("id")
}

fn shell() -> MockShell {
    let row =
        |id: &str, name: &str, age: i64| rec(&[("id", t(id)), ("name", t(name)), ("age", i(age))]);
    MockShell::new()
        .with_metadata(metadata())
        .with_record("p1", row("p1", "Ann", 30))
        .with_record("p2", row("p2", "Bob", 20))
        .with_record("p3", row("p3", "Cy", 40))
}

fn people() -> Vista {
    Vista::new("people", shell())
}

fn limited(caps: &[Capability]) -> Vista {
    let set: VistaCapabilities = caps.iter().copied().collect();
    Vista::new("people", shell().with_capabilities(set))
}

async fn ids(vista: &Vista) -> Vec<String> {
    vista.list_values().await.unwrap().into_keys().collect()
}

fn refused(err: &vantage_core::VantageError) -> bool {
    err.is_unsupported() || err.is_unimplemented()
}

#[test]
fn a_vista_reports_its_schema_and_name() {
    let vista = people();
    assert_eq!(vista.name(), "people");
    assert_eq!(vista.clone().with_name("folk").name(), "folk");
    assert_eq!(vista.id_column(), Some("id"));
    assert_eq!(vista.title_columns(), ["name"]);
    assert_eq!(vista.columns().len(), 5);
    assert!(vista.column("age").is_some());
    assert!(vista.column("nope").is_none());
    assert!(vista.references().is_empty());
    assert!(vista.reference("nope").is_none());
    assert!(!vista.driver_name().is_empty());
    assert!(vista.capabilities().has(Capability::Insert));
}

#[tokio::test]
async fn a_declared_id_generator_survives_narrowing_and_wrapping() {
    assert!(people().id_generator().is_none(), "none declared: UUIDv7");
    let vista = people()
        .with_id_generator(IdGenerator::short(6, "ab").unwrap())
        .with_condition("age", FilterOp::Gt, 25_i64)
        .unwrap()
        .map_shell(|inner| Wrap { inner });
    let id = vista.id_generator().expect("generator kept").generate();
    assert_eq!(id.len(), 6, "{id}");
    assert!(id.chars().all(|c| c == 'a' || c == 'b'), "{id}");
    assert_eq!(
        ids(&vista).await,
        ["p1", "p3"],
        "reads still reach the shell"
    );

    let declared = Vista::new(
        "people",
        shell().with_metadata(metadata().with_id_generator(IdGenerator::UuidV4)),
    );
    assert!(matches!(declared.id_generator(), Some(IdGenerator::UuidV4)));
}

#[tokio::test]
async fn an_operator_condition_narrows_and_is_recorded() {
    let vista = people()
        .with_condition("age", FilterOp::Gt, 25_i64)
        .unwrap();
    assert!(vista.is_narrowed());
    assert_eq!(
        vista.query().conditions,
        [Condition::new("age", FilterOp::Gt, 25_i64)]
    );
    assert_eq!(ids(&vista).await, ["p1", "p3"]);
    assert_eq!(vista.count().await.unwrap(), 2);
}

#[test]
fn with_id_needs_an_id_column_and_ordering_errors_surface_through_with_order() {
    let bare = Vista::new("bare", MockShell::new().with_metadata(VistaMetadata::new()));
    assert!(bare.with_id("x").is_err());
    assert!(
        people()
            .with_order("note", SortDirection::Ascending)
            .is_err()
    );
    assert!(people().with_search("x").is_ok());
}

#[cfg(feature = "rhai")]
#[tokio::test]
async fn a_computed_column_that_fails_fails_the_read_it_belongs_to() {
    let failing = Column::new("boom", "i64")
        .with_expression("row.nothing.explode()")
        .unwrap();
    let ok = Column::new("double", "i64")
        .with_expression("row.age * 2")
        .unwrap();
    let meta = metadata().with_column(ok).with_column(failing);
    let vista = Vista::new("people", shell().with_metadata(meta));
    assert!(vista.list_values().await.is_err());

    let meta = metadata().with_column(
        Column::new("double", "i64")
            .with_expression("row.age * 2")
            .unwrap(),
    );
    let vista = Vista::new("people", shell().with_metadata(meta));
    let row = vista.get_value("p2").await.unwrap().unwrap();
    assert_eq!(row.get("double"), Some(&i(40)));
}

#[tokio::test]
async fn with_id_narrows_to_the_row_whose_id_column_equals_it() {
    let vista = people().with_id("p2").unwrap();
    assert_eq!(ids(&vista).await, ["p2"]);
    assert_eq!(vista.query().conditions[0].column, "id");
}

#[test]
fn an_operator_beyond_equality_needs_filter_ops() {
    let vista = limited(&[]);
    let err = vista
        .with_condition("age", FilterOp::Gt, 1_i64)
        .unwrap_err();
    assert!(refused(&err), "{err:?}");
    let ok = limited(&[])
        .with_condition("age", FilterOp::Eq, 30_i64)
        .unwrap();
    assert!(ok.is_narrowed());
}

#[test]
fn an_unknown_column_is_a_plain_error_not_a_refusal() {
    let err = people()
        .with_condition("nope", FilterOp::Gt, 1_i64)
        .unwrap_err();
    assert!(!refused(&err), "{err:?}");
    let err = limited(&[])
        .with_condition("nope", FilterOp::Gt, 1_i64)
        .unwrap_err();
    assert!(
        !refused(&err),
        "an unknown column is checked before the operator: {err:?}"
    );
}

#[test]
fn a_refused_clause_is_not_recorded() {
    let mut vista = limited(&[]);
    assert!(vista.set_search(Some("x")).is_err());
    assert!(vista.query().is_empty());
    assert!(!vista.is_narrowed());
}

#[tokio::test]
async fn a_dotted_path_into_a_declared_column_is_valid_or_refused_never_unknown() {
    let vista = people();
    let err = vista
        .clone()
        .with_condition("dims.w", FilterOp::Eq, 1_i64)
        .unwrap_err();
    assert!(refused(&err), "the mock refuses paths: {err:?}");
    assert!(vista.query().is_empty());
    let err = people()
        .with_condition("nope.w", FilterOp::Eq, 1_i64)
        .unwrap_err();
    assert!(!refused(&err), "{err:?}");
}

#[tokio::test]
async fn search_replaces_and_clears() {
    let mut vista = people();
    vista.set_search(Some("an")).unwrap();
    assert_eq!(vista.query().search.as_deref(), Some("an"));
    assert_eq!(ids(&vista).await, ["p1"]);
    vista.set_search(Some("Bob")).unwrap();
    assert_eq!(vista.query().search.as_deref(), Some("Bob"));
    assert_eq!(ids(&vista).await, ["p2"]);
    vista.set_search(None).unwrap();
    assert!(vista.query().search.is_none());
    assert_eq!(ids(&vista).await.len(), 3);

    let mut untouched = people();
    untouched.set_search(None).unwrap();
    assert!(untouched.query().is_empty());

    let searched = people().with_search("cy").unwrap();
    assert_eq!(ids(&searched).await, ["p3"]);
}

#[tokio::test]
async fn order_replaces_clears_and_does_not_narrow() {
    let mut vista = people();
    vista
        .set_order(Some(("age", SortDirection::Ascending)))
        .unwrap();
    assert_eq!(ids(&vista).await, ["p2", "p1", "p3"]);
    assert!(!vista.is_narrowed());
    vista
        .set_order(Some(("age", SortDirection::Descending)))
        .unwrap();
    assert_eq!(ids(&vista).await, ["p3", "p1", "p2"]);
    vista.set_order(None).unwrap();
    assert!(vista.query().order.is_none());

    let mut untouched = people();
    untouched.set_order(None).unwrap();
    assert!(untouched.query().is_empty());

    let ordered = people()
        .with_order("name", SortDirection::Descending)
        .unwrap();
    assert_eq!(ids(&ordered).await, ["p3", "p2", "p1"]);
}

#[test]
fn ordering_needs_the_capability_and_an_orderable_declared_column() {
    let mut no_order = limited(&[]);
    let err = no_order
        .set_order(Some(("age", SortDirection::Ascending)))
        .unwrap_err();
    assert!(refused(&err), "{err:?}");

    let mut vista = people();
    let err = vista
        .set_order(Some(("note", SortDirection::Ascending)))
        .unwrap_err();
    assert!(refused(&err), "an unflagged column is refused: {err:?}");
    let err = vista
        .set_order(Some(("nope", SortDirection::Ascending)))
        .unwrap_err();
    assert!(
        !refused(&err),
        "an undeclared column is a plain error: {err:?}"
    );
    assert!(vista.query().is_empty());
}

#[test]
fn a_native_condition_is_refused_by_a_shell_without_the_capability() {
    let mut vista = people();
    let err = vista
        .add_native_condition(NativeCondition::new("raw", "raw"))
        .unwrap_err();
    assert!(refused(&err), "{err:?}");
    assert!(vista.query().native.is_empty());
    assert!(
        people()
            .with_native_condition(NativeCondition::new("raw", "raw"))
            .is_err()
    );
}

#[tokio::test]
async fn with_query_applies_what_the_shell_takes_and_returns_the_rest() {
    let query = VistaQuery::new()
        .with_condition(Condition::new("age", FilterOp::Gt, 25_i64))
        .with_search("a")
        .with_order("age", SortDirection::Ascending);
    let (vista, residual) = people().with_query(&query).unwrap();
    assert!(residual.is_empty(), "{residual:?}");
    assert_eq!(vista.query(), &query);
    assert_eq!(ids(&vista).await, ["p1"]);

    let (vista, residual) = limited(&[Capability::Window]).with_query(&query).unwrap();
    assert!(vista.query().is_empty());
    assert_eq!(residual.conditions, query.conditions);
    assert_eq!(residual.search.as_deref(), Some("a"));
    assert_eq!(residual.order, query.order);
}

#[test]
fn with_query_returns_other_errors_and_cannot_reapply_native_keys() {
    let unknown = VistaQuery::new().with_condition(Condition::new("nope", FilterOp::Gt, 1_i64));
    assert!(people().with_query(&unknown).is_err());

    let mut native = VistaQuery::new();
    native.native.push("raw".into());
    let err = people().with_query(&native).unwrap_err();
    assert!(err.is_incorrect_usage(), "{err:?}");
}

#[tokio::test]
async fn map_shell_keeps_the_name_and_the_applied_query() {
    let vista = people()
        .with_condition("age", FilterOp::Gt, 25_i64)
        .unwrap();
    let query = vista.query().clone();
    let wrapped = vista.map_shell(|inner| Wrap { inner });
    assert_eq!(wrapped.name(), "people");
    assert_eq!(wrapped.query(), &query);
    assert_eq!(ids(&wrapped).await, ["p1", "p3"]);
}

#[test]
fn preview_describes_the_next_read() {
    let preview = people().preview();
    assert!(preview.query.is_some() || preview.note.is_some());
}

#[tokio::test]
async fn windows_past_the_start_need_the_window_capability() {
    let page = people().fetch_window(1, 1).await.unwrap();
    assert_eq!(page.rows.len(), 1);
    assert_eq!(page.rows[0].0, "p2");
    assert_eq!(page.total, Some(3));

    let lead = limited(&[]).fetch_window(0, 2).await.unwrap();
    assert_eq!(lead.rows.len(), 2);
    let err = limited(&[]).fetch_window(1, 1).await.unwrap_err();
    assert!(err.is_unsupported(), "{err:?}");
}

#[tokio::test]
async fn a_forwarding_wrapper_passes_cursor_and_watch_to_the_inner_shell() {
    let caps: VistaCapabilities = [Capability::Cursor, Capability::Subscribe]
        .into_iter()
        .collect();
    let vista =
        Vista::new("people", shell().with_capabilities(caps)).map_shell(|inner| Wrap { inner });
    let err = vista.fetch_cursor(None, 1).await.unwrap_err();
    assert!(err.is_unsupported(), "the mock has no cursor: {err:?}");
    let err = vista
        .watch()
        .await
        .err()
        .expect("the mock has no live feed");
    assert!(err.is_unsupported(), "{err:?}");
    assert_eq!(vista.count().await.unwrap(), 3);
}

#[tokio::test]
async fn cursor_and_watch_are_refused_without_their_capabilities() {
    let err = people().fetch_cursor(None, 1).await.unwrap_err();
    assert!(err.is_unsupported(), "{err:?}");
    let err = people().watch().await.err().expect("refused");
    assert!(err.is_unsupported(), "{err:?}");
}

#[tokio::test]
async fn a_write_is_refused_unless_the_shell_advertises_it() {
    let read_only = limited(&[]);
    let row = rec(&[("name", t("Di"))]);
    let err = read_only.insert_return_id_value(&row).await.unwrap_err();
    assert!(err.is_unsupported(), "{err:?}");
    let err = read_only
        .insert_value("p9".to_string(), &row)
        .await
        .unwrap_err();
    assert!(err.is_unsupported(), "{err:?}");
    let err = read_only.delete("p1".to_string()).await.unwrap_err();
    assert!(err.is_unsupported(), "{err:?}");
    let err = read_only.upsert_value("p1", &row).await.unwrap_err();
    assert!(err.is_unsupported(), "{err:?}");
    let err = read_only
        .patch_value("p1".to_string(), &row)
        .await
        .unwrap_err();
    assert!(err.is_unsupported(), "{err:?}");
    assert_eq!(read_only.count().await.unwrap(), 3);
}

#[tokio::test]
async fn delete_all_needs_its_own_capability() {
    let vista = limited(&[Capability::Delete]);
    vista.delete("p1".to_string()).await.unwrap();
    let err = vista.delete_all().await.unwrap_err();
    assert!(err.is_unsupported(), "{err:?}");
    assert_eq!(vista.count().await.unwrap(), 2);
}

#[tokio::test]
async fn an_advertised_write_still_runs() {
    let vista = people();
    let id = vista
        .insert_return_id_value(&rec(&[("name", t("Di")), ("age", i(5))]))
        .await
        .unwrap();
    assert_eq!(vista.count().await.unwrap(), 4);
    vista
        .upsert_value("p1", &rec(&[("id", t("p1")), ("name", t("Anna"))]))
        .await
        .unwrap();
    assert_eq!(
        vista.get_value("p1").await.unwrap().unwrap().get("name"),
        Some(&t("Anna"))
    );
    vista.delete(&id).await.unwrap();
    assert_eq!(vista.count().await.unwrap(), 3);
}

#[tokio::test]
async fn writes_through_a_narrowed_vista_need_a_shell_that_confines_them() {
    let caps: VistaCapabilities = Capability::iter()
        .filter(|c| *c != Capability::ConfineWrites)
        .collect();
    let vista = Vista::new("people", shell().with_capabilities(caps))
        .with_condition("age", FilterOp::Gt, 25_i64)
        .unwrap();
    let err = vista
        .insert_return_id_value(&rec(&[("name", t("Di")), ("age", i(50))]))
        .await
        .unwrap_err();
    assert!(refused(&err), "{err:?}");

    let confining = people()
        .with_condition("age", FilterOp::Gt, 25_i64)
        .unwrap();
    assert!(
        confining
            .insert_return_id_value(&rec(&[("name", t("Di")), ("age", i(50))]))
            .await
            .is_ok()
    );
}

#[tokio::test]
async fn get_ref_of_an_undeclared_relation_is_not_found() {
    let err = people().get_ref("nope", None).unwrap_err();
    assert!(err.is_not_found(), "{err:?}");
    let _ = CborValue::Null;
}

#[tokio::test]
async fn import_needs_the_import_capability() {
    let err = people()
        .import_values(&IndexMap::from([(
            "p9".to_string(),
            rec(&[("name", t("Di"))]),
        )]))
        .await
        .unwrap_err();
    assert!(err.is_unsupported(), "{err:?}");
}
