//! Join and contained traversal.

use ciborium::Value as CborValue;
use vantage_table::table::Table;
use vantage_types::EmptyEntity;
use vantage_vista::{FilterOp, VistaFactory};

use super::{BAKERY, clients, factory, factory_with, query, row, spec, test_db};
use crate::surrealdb::SurrealDB;
use crate::types::AnySurrealType;
use crate::vista::SurrealVistaFactory;

#[test]
fn has_many_narrows_by_a_coerced_parent_id() {
    let bakeries = factory().build_from_spec(spec(BAKERY)).unwrap();
    let parent = row(&[("id", "bakery:hill_valley".into())]);
    let clients = bakeries.get_ref("clients", Some(&parent)).unwrap();
    assert!(clients.is_narrowed());
    assert_eq!(clients.query().conditions[0].column, "bakery");
    assert!(
        query(&clients).contains("bakery = bakery:hill_valley"),
        "the string id is a record literal: {}",
        query(&clients)
    );

    let bare = bakeries.get_ref("clients", None).unwrap();
    assert!(!bare.is_narrowed());
}

/// The documented link shape: a `string` column with a column-level
/// `references:`, which the loader lowers into the has-one declared below.
const STRING_LINKED_CLIENT: &str = r#"
name: client
columns:
  id: { type: string, flags: [id] }
  city: { type: string }
  bakery: { type: string, references: bakery }
references:
  bakery: { table: bakery, kind: has_one, foreign_key: bakery }
"#;

#[test]
fn a_has_one_link_column_takes_an_id_held_as_text() {
    let factory = factory_with(vec![("client", STRING_LINKED_CLIENT), ("bakery", BAKERY)]);
    let bakeries = factory.build_from_spec(spec(BAKERY)).unwrap();
    let parent = row(&[("id", "bakery:hill_valley".into())]);
    let clients = bakeries.get_ref("clients", Some(&parent)).unwrap();
    assert!(
        query(&clients).contains("bakery = bakery:hill_valley"),
        "a record literal through traversal: {}",
        query(&clients)
    );

    let direct = factory
        .build_from_spec(spec(STRING_LINKED_CLIENT))
        .unwrap()
        .with_condition("city", FilterOp::Eq, "a:b")
        .unwrap();
    assert!(
        query(&direct).contains("city = \"a:b\""),
        "a plain string column keeps its text: {}",
        query(&direct)
    );
}

#[test]
fn has_one_narrows_by_the_target_id() {
    let parent = row(&[
        ("id", "client:marty".into()),
        ("bakery", "bakery:hv".into()),
    ]);
    let bakery = clients().get_ref("bakery", Some(&parent)).unwrap();
    assert_eq!(bakery.query().conditions[0].column, "id");
    assert!(
        query(&bakery).contains("id = bakery:hv"),
        "{}",
        query(&bakery)
    );

    let err = clients()
        .get_ref("nope", None)
        .map(drop)
        .expect_err("unknown relation");
    assert!(err.is_not_found(), "{err:?}");
}

#[test]
fn contained_relation_without_a_row_is_its_bare_target() {
    let table = Table::<SurrealDB, EmptyEntity>::new("contained_test", test_db())
        .with_id_column("id")
        .with_column_of::<AnySurrealType>("lines")
        .with_contained_many(
            "lines",
            "lines",
            |db| Table::new("lines", db).with_column_of::<i64>("a"),
            None,
        );
    let vista = SurrealVistaFactory::new(test_db())
        .from_table(table)
        .unwrap();
    assert!(
        vista
            .reference("lines")
            .is_some_and(|r| r.kind.is_contained())
    );

    // Without a parent row, the bare target: the schema and no writes.
    let bare = vista.get_ref("lines", None).unwrap();
    assert!(bare.column("a").is_some());
    assert!(!bare.capabilities().has(vantage_vista::Capability::Insert));

    let lines = CborValue::Array(vec![CborValue::Map(vec![("a".into(), 1.into())])]);
    let parent = row(&[("id", "contained_test:t1".into()), ("lines", lines)]);
    assert!(vista.get_ref("lines", Some(&parent)).is_ok());
}

/// A contained record keyed by its relation's `id_column` narrows a
/// has-many target by that column, not by the contained table's `id`.
#[test]
fn contained_has_many_reads_the_relation_id_column() {
    let notes =
        |db| Table::<SurrealDB, EmptyEntity>::new("note", db).with_column_of::<String>("line");
    let table = Table::<SurrealDB, EmptyEntity>::new("invoice", test_db())
        .with_id_column("id")
        .with_column_of::<AnySurrealType>("lines")
        .with_contained_many(
            "lines",
            "lines",
            move |db| {
                Table::new("lines", db)
                    .with_column_of::<String>("line_no")
                    .with_many("notes", "line", notes)
            },
            Some("line_no"),
        );
    let vista = SurrealVistaFactory::new(test_db())
        .from_table(table)
        .unwrap();
    let line = CborValue::Map(vec![("line_no".into(), "L7".into())]);
    let parent = row(&[
        ("id", "invoice:i1".into()),
        ("lines", CborValue::Array(vec![line])),
    ]);
    let lines = vista.get_ref("lines", Some(&parent)).unwrap();
    let notes = lines
        .get_ref("notes", Some(&row(&[("line_no", "L7".into())])))
        .unwrap();
    let condition = &notes.query().conditions[0];
    assert_eq!(condition.column, "line");
    assert_eq!(condition.value, CborValue::from("L7"));
}
