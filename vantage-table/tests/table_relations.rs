#![cfg(feature = "mocks")]

//! Relations declared with `with_one` / `with_many`, and traversing them.
//!
//! `with_one` and `with_many` need a source whose value type converts to and
//! from `ciborium::Value`, so these tests run on a small CBOR-valued source
//! that stores nothing: relations only build conditions and metadata.

use async_trait::async_trait;
use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_core::Result;
use vantage_expressions::{
    Expression,
    traits::associated_expressions::AssociatedExpression,
    traits::datasource::{DataSource, ExprDataSource},
    traits::expressive::ExpressiveEnum,
};
use vantage_table::column::core::{Column, ColumnType};
use vantage_table::conditions::literal_equality;
use vantage_table::prelude::*;
use vantage_table::table::VistaMetadataOptions;
use vantage_types::{EmptyEntity, Entity, Record};
use vantage_vista::{JoinKey, Reference, ReferenceKind};

#[derive(Clone)]
struct CborSource;

impl DataSource for CborSource {}

impl ExprDataSource<CborValue> for CborSource {
    async fn execute(&self, _expr: &Expression<CborValue>) -> Result<CborValue> {
        unimplemented!("the relation tests never run a query")
    }

    fn defer(
        &self,
        _expr: Expression<CborValue>,
    ) -> vantage_expressions::traits::expressive::DeferredFn<CborValue> {
        unimplemented!("the relation tests never run a query")
    }
}

fn refused<T>() -> Result<T> {
    Err(vantage_core::error!("this source stores nothing"))
}

#[async_trait]
impl TableSource for CborSource {
    type Column<Type>
        = Column<Type>
    where
        Type: ColumnType;
    type AnyType = CborValue;
    type Value = CborValue;
    type Id = String;
    type Condition = Expression<CborValue>;
    type Source = String;

    fn condition_equality(&self, condition: &Expression<CborValue>) -> Option<(String, CborValue)> {
        literal_equality(condition)
    }

    fn eq_value_condition(&self, field: &str, value: CborValue) -> Result<Expression<CborValue>> {
        Ok(Expression::new(
            "{} = {}",
            vec![
                ExpressiveEnum::Nested(Expression::new(field.to_string(), vec![])),
                ExpressiveEnum::Nested(Expression::new("{}", vec![ExpressiveEnum::Scalar(value)])),
            ],
        ))
    }

    fn create_column<Type: ColumnType>(&self, name: &str) -> Column<Type> {
        Column::new(name)
    }

    fn to_any_column<Type: ColumnType>(&self, column: Column<Type>) -> Column<CborValue> {
        Column::from_column(column)
    }

    fn convert_any_column<Type: ColumnType>(
        &self,
        any_column: Column<CborValue>,
    ) -> Option<Column<Type>> {
        Some(Column::from_column(any_column))
    }

    fn expr(
        &self,
        template: impl Into<String>,
        parameters: Vec<ExpressiveEnum<CborValue>>,
    ) -> Expression<CborValue> {
        Expression::new(template, parameters)
    }

    fn search_table_condition<E>(
        &self,
        _table: &Table<Self, E>,
        _search_value: &str,
    ) -> Expression<CborValue>
    where
        E: Entity<CborValue>,
    {
        Expression::new("search", vec![])
    }

    async fn list_table_values<E>(
        &self,
        _table: &Table<Self, E>,
    ) -> Result<IndexMap<String, Record<CborValue>>>
    where
        E: Entity<CborValue>,
        Self: Sized,
    {
        refused()
    }

    async fn get_table_value<E>(
        &self,
        _table: &Table<Self, E>,
        _id: &String,
    ) -> Result<Option<Record<CborValue>>>
    where
        E: Entity<CborValue>,
        Self: Sized,
    {
        refused()
    }

    async fn get_table_some_value<E>(
        &self,
        _table: &Table<Self, E>,
    ) -> Result<Option<(String, Record<CborValue>)>>
    where
        E: Entity<CborValue>,
        Self: Sized,
    {
        refused()
    }

    async fn get_table_count<E>(&self, _table: &Table<Self, E>) -> Result<i64>
    where
        E: Entity<CborValue>,
        Self: Sized,
    {
        refused()
    }

    async fn get_table_sum<E>(
        &self,
        _table: &Table<Self, E>,
        _column: &Column<CborValue>,
    ) -> Result<CborValue>
    where
        E: Entity<CborValue>,
        Self: Sized,
    {
        refused()
    }

    async fn get_table_max<E>(
        &self,
        _table: &Table<Self, E>,
        _column: &Column<CborValue>,
    ) -> Result<CborValue>
    where
        E: Entity<CborValue>,
        Self: Sized,
    {
        refused()
    }

    async fn get_table_min<E>(
        &self,
        _table: &Table<Self, E>,
        _column: &Column<CborValue>,
    ) -> Result<CborValue>
    where
        E: Entity<CborValue>,
        Self: Sized,
    {
        refused()
    }

    async fn insert_table_value<E>(
        &self,
        _table: &Table<Self, E>,
        _id: &String,
        _record: &Record<CborValue>,
    ) -> Result<Record<CborValue>>
    where
        E: Entity<CborValue>,
        Self: Sized,
    {
        refused()
    }

    async fn replace_table_value<E>(
        &self,
        _table: &Table<Self, E>,
        _id: &String,
        _record: &Record<CborValue>,
    ) -> Result<Record<CborValue>>
    where
        E: Entity<CborValue>,
        Self: Sized,
    {
        refused()
    }

    async fn patch_table_value<E>(
        &self,
        _table: &Table<Self, E>,
        _id: &String,
        _partial: &Record<CborValue>,
    ) -> Result<Record<CborValue>>
    where
        E: Entity<CborValue>,
        Self: Sized,
    {
        refused()
    }

    async fn delete_table_value<E>(&self, _table: &Table<Self, E>, _id: &String) -> Result<()>
    where
        E: Entity<CborValue>,
        Self: Sized,
    {
        refused()
    }

    async fn delete_table_all_values<E>(&self, _table: &Table<Self, E>) -> Result<()>
    where
        E: Entity<CborValue>,
        Self: Sized,
    {
        refused()
    }

    async fn insert_table_return_id_value<E>(
        &self,
        _table: &Table<Self, E>,
        _record: &Record<CborValue>,
    ) -> Result<String>
    where
        E: Entity<CborValue>,
        Self: Sized,
    {
        refused()
    }

    fn related_in_condition<SourceE: Entity<CborValue> + 'static>(
        &self,
        target_field: &str,
        source_table: &Table<Self, SourceE>,
        source_column: &str,
    ) -> Expression<CborValue>
    where
        Self: Sized,
    {
        Expression::new(
            format!(
                "{target_field} IN (SELECT {source_column} FROM {})",
                source_table.table_name()
            ),
            vec![],
        )
    }

    fn related_correlated_condition(
        &self,
        target_table: &str,
        target_field: &str,
        source_table: &str,
        source_column: &str,
    ) -> Expression<CborValue> {
        Expression::new(
            format!("{target_table}.{target_field} = {source_table}.{source_column}"),
            vec![],
        )
    }

    fn column_table_values_expr<'a, E, Type: ColumnType>(
        &'a self,
        _table: &Table<Self, E>,
        _column: &Column<Type>,
    ) -> AssociatedExpression<'a, Self, CborValue, Vec<Type>>
    where
        E: Entity<CborValue> + 'static,
        Self: Sized,
    {
        unimplemented!("the relation tests never run a query")
    }
}

type CborTable = Table<CborSource, EmptyEntity>;

fn bakery(src: CborSource) -> CborTable {
    Table::new("bakery", src)
        .with_id_column("id")
        .with_column_of::<String>("name")
}

fn order(src: CborSource) -> CborTable {
    Table::new("client_order", src)
        .with_id_column("id")
        .with_column_of::<String>("client_id")
}

fn client() -> CborTable {
    Table::new("client", CborSource)
        .with_id_column("id")
        .with_column_of::<String>("name")
        .with_column_of::<String>("bakery_id")
        .with_one("bakery", "bakery_id", bakery)
        .with_many("orders", "client_id", order)
}

fn row(pairs: &[(&str, &str)]) -> Record<CborValue> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), CborValue::Text(v.to_string())))
        .collect()
}

fn text(s: &str) -> CborValue {
    CborValue::Text(s.to_string())
}

// --- declared relations --------------------------------------------------------

#[test]
fn a_table_without_relations_lists_none() {
    let table: CborTable = Table::new("client", CborSource);
    assert!(table.references().is_empty());
    assert!(table.ref_kinds().is_empty());
    assert!(table.vista_references().is_empty());
}

#[test]
fn relations_are_listed_in_declaration_order_with_their_cardinality() {
    let table = client();
    assert_eq!(table.references(), ["bakery", "orders"]);
    assert_eq!(
        table.ref_kinds(),
        [
            ("bakery".to_string(), ReferenceKind::HasOne),
            ("orders".to_string(), ReferenceKind::HasMany),
        ]
    );
    assert_eq!(
        table.ref_cardinality("bakery").unwrap(),
        ReferenceKind::HasOne
    );
    assert_eq!(
        table.ref_cardinality("orders").unwrap(),
        ReferenceKind::HasMany
    );
}

#[test]
fn a_has_ones_foreign_key_is_the_source_column_and_a_has_manys_is_the_target_column() {
    let table = client();
    assert_eq!(table.ref_foreign_key("bakery").unwrap(), "bakery_id");
    assert_eq!(table.ref_foreign_key("orders").unwrap(), "client_id");
}

#[test]
fn asking_about_an_unknown_relation_is_an_error() {
    let table = client();
    assert!(table.ref_cardinality("nope").is_err());
    assert!(table.ref_foreign_key("nope").is_err());
    let bare: CborTable = Table::new("client", CborSource);
    assert!(bare.ref_cardinality("orders").is_err());
}

#[test]
fn vista_references_name_the_target_table_and_the_join_key() {
    assert_eq!(
        client().vista_references(),
        [
            Reference::has_one("bakery", "bakery", JoinKey::to_target_id("bakery_id")),
            Reference::has_many("orders", "client_order", JoinKey::new("id", "client_id")),
        ]
    );
}

#[test]
fn a_has_many_join_key_starts_from_the_tables_own_id_column() {
    let table: CborTable = Table::new("client", CborSource)
        .with_id_column("code")
        .with_many("orders", "client_code", order);
    assert_eq!(
        table.vista_references(),
        [Reference::has_many(
            "orders",
            "client_order",
            JoinKey::new("code", "client_code")
        )]
    );
}

#[test]
fn vista_metadata_folds_in_references_only_when_asked() {
    let without = client().vista_metadata(VistaMetadataOptions::default());
    assert!(without.references.is_empty());

    let with = client().vista_metadata(VistaMetadataOptions {
        references: true,
        ..VistaMetadataOptions::default()
    });
    assert_eq!(
        with.references
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        ["bakery", "orders"]
    );
}

#[test]
fn copy_relations_from_shares_all_or_only_the_listed_relations() {
    let source = client();
    let mut all: CborTable = Table::new("copy", CborSource);
    all.copy_relations_from(&source, None);
    assert_eq!(all.references(), ["bakery", "orders"]);

    let mut some: CborTable = Table::new("copy", CborSource);
    some.copy_relations_from(&source, Some(&["orders"]));
    assert_eq!(some.references(), ["orders"]);

    let mut none: CborTable = Table::new("copy", CborSource);
    none.copy_relations_from(
        &Table::<CborSource, EmptyEntity>::new("bare", CborSource),
        None,
    );
    assert!(none.references().is_empty());
}

// --- traversal ----------------------------------------------------------------

fn invariant_of(table: &CborTable, column: &str) -> Option<CborValue> {
    table.invariants().get(column).cloned()
}

#[test]
fn a_has_many_from_a_row_reads_the_sources_id_and_narrows_the_target_by_it() {
    let orders = client()
        .get_ref_from_row::<EmptyEntity>("orders", &row(&[("id", "c1")]))
        .unwrap();
    assert_eq!(orders.table_name(), "client_order");
    let conditions: Vec<_> = orders.conditions().collect();
    assert_eq!(conditions.len(), 1);
    assert_eq!(
        literal_equality(conditions[0]),
        Some(("client_id".to_string(), text("c1")))
    );
    assert_eq!(invariant_of(&orders, "client_id"), Some(text("c1")));
}

#[test]
fn a_has_one_from_a_row_reads_its_foreign_key_and_narrows_the_targets_id() {
    let bakery = client()
        .get_ref_from_row::<EmptyEntity>("bakery", &row(&[("id", "c1"), ("bakery_id", "b7")]))
        .unwrap();
    assert_eq!(bakery.table_name(), "bakery");
    let conditions: Vec<_> = bakery.conditions().collect();
    assert_eq!(conditions.len(), 1);
    assert_eq!(
        literal_equality(conditions[0]),
        Some(("id".to_string(), text("b7")))
    );
    assert_eq!(invariant_of(&bakery, "id"), Some(text("b7")));
}

#[test]
fn a_row_without_the_join_field_is_an_error() {
    let has_many = client().get_ref_from_row::<EmptyEntity>("orders", &row(&[("name", "x")]));
    assert!(has_many.is_err());
    let has_one = client().get_ref_from_row::<EmptyEntity>("bakery", &row(&[("id", "c1")]));
    assert!(has_one.is_err());
}

#[test]
fn traversing_an_unknown_relation_from_a_row_is_an_error() {
    assert!(
        client()
            .get_ref_from_row::<EmptyEntity>("nope", &row(&[("id", "c1")]))
            .is_err()
    );
    assert!(client().get_ref_as::<EmptyEntity>("nope").is_err());
    assert!(client().get_subquery_as::<EmptyEntity>("nope").is_err());
    assert!(client().get_ref_target_erased("nope").is_err());
}

#[test]
fn get_ref_as_narrows_the_target_by_the_sources_set() {
    let orders = client().get_ref_as::<EmptyEntity>("orders").unwrap();
    assert_eq!(orders.table_name(), "client_order");
    let conditions: Vec<&str> = orders.conditions().map(|c| c.template.as_str()).collect();
    assert_eq!(conditions, ["client_id IN (SELECT id FROM client)"]);
}

#[test]
fn get_ref_as_from_a_has_one_matches_the_targets_id_to_the_sources_foreign_key() {
    let bakery = client().get_ref_as::<EmptyEntity>("bakery").unwrap();
    let conditions: Vec<&str> = bakery.conditions().map(|c| c.template.as_str()).collect();
    assert_eq!(conditions, ["id IN (SELECT bakery_id FROM client)"]);
}

#[test]
fn get_subquery_as_correlates_the_target_with_the_source() {
    let orders = client().get_subquery_as::<EmptyEntity>("orders").unwrap();
    let conditions: Vec<&str> = orders.conditions().map(|c| c.template.as_str()).collect();
    assert_eq!(conditions, ["client_order.client_id = client.id"]);
}

#[test]
fn get_subquery_erased_builds_the_same_correlated_target() {
    let orders = client().get_subquery_erased("orders").unwrap();
    let conditions: Vec<&str> = orders.conditions().map(|c| c.template.as_str()).collect();
    assert_eq!(conditions, ["client_order.client_id = client.id"]);
    assert_eq!(orders.table_name(), "client_order");
}

#[test]
fn get_ref_target_erased_is_the_bare_target_with_no_condition() {
    let bakery = client().get_ref_target_erased("bakery").unwrap();
    assert_eq!(bakery.table_name(), "bakery");
    assert_eq!(bakery.conditions().count(), 0);
    assert!(bakery.invariants().is_empty());
    assert!(bakery.columns().contains_key("name"));
}

#[test]
fn with_id_narrows_to_the_id_and_registers_it_as_an_invariant() {
    let one = client().with_id(CborValue::Text("c1".into())).unwrap();
    let conditions: Vec<_> = one.conditions().collect();
    assert_eq!(
        literal_equality(conditions[0]),
        Some(("id".to_string(), text("c1")))
    );
    assert_eq!(invariant_of(&one, "id"), Some(text("c1")));
}
