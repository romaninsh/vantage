//! Every SQL dialect describes a condition as its rendered expression and
//! names the column an equality or a traversal `IN (…)` constrains; a raw or
//! combined expression constrains no known column, nor does a negation or a
//! non-value right side. `in_values_condition` narrows to its listed values
//! and names its column.

use vantage_dataset::ReadableValueSet;
use vantage_table::column::core::ColumnType;
use vantage_table::table::Table;
use vantage_table::traits::table_source::TableSource;
use vantage_types::EmptyEntity;

/// `p1` is the dialect's value for the text `p1`; `raw` is the expression
/// `1 = 1` and `both` an `AND` of two equalities, built with the dialect's
/// expression macro.
pub fn assert_condition_text_and_column<T: TableSource>(
    db: &T,
    p1: T::Value,
    raw: T::Condition,
    both: T::Condition,
) where
    T::Id: ColumnType,
{
    let eq = db.eq_value_condition("parent", p1).unwrap();
    assert_eq!(db.describe_condition(&eq), "parent = 'p1'");
    assert_eq!(db.condition_column(&eq), Some("parent".to_string()));

    let invoices = Table::<T, EmptyEntity>::new("invoice", db.clone()).with_id_column("id");
    let in_cond = db.related_in_condition("invoice_id", &invoices, "id");
    assert_eq!(
        db.condition_column(&in_cond),
        Some("invoice_id".to_string())
    );

    assert_eq!(db.condition_column(&raw), None);
    assert_eq!(db.condition_column(&both), None);
}

/// The seeded `product` table narrowed by `id IN (flux_cupcake,
/// delorean_donut)` lists exactly those two rows; an empty list, none.
/// `id` is the dialect's value for a text id.
pub async fn assert_in_values_narrows_products<T: TableSource>(db: &T, id: fn(&str) -> T::Value)
where
    T::Id: ColumnType + std::fmt::Display,
{
    let products = |values: Vec<T::Value>| {
        let condition = db.in_values_condition("id", values).unwrap();
        Table::<T, EmptyEntity>::new("product", db.clone())
            .with_id_column("id")
            .with_column_of::<String>("name")
            .with_condition(condition)
    };
    let listed = products(vec![id("flux_cupcake"), id("delorean_donut")])
        .list_values()
        .await
        .unwrap();
    let mut ids: Vec<String> = listed.keys().map(|k| k.to_string()).collect();
    ids.sort();
    assert_eq!(ids, ["delorean_donut", "flux_cupcake"]);

    let none = products(vec![]).list_values().await.unwrap();
    assert!(none.is_empty());
}

/// Conditions built with the dialect's expression macro: `gt` is
/// `invoice_id > 1`; the rest are on `tenant_id` and narrow it to no value.
pub struct NarrowingShapes<C> {
    /// `!=`, `<>`, `NOT IN (…)`, `NOT LIKE`.
    pub negations: Vec<C>,
    pub gt: C,
    /// `tenant_id = tenant_id`.
    pub column_vs_column: C,
    /// `tenant_id = 7 OR 1 = 1`, the right side a nested raw expression.
    pub or_operand: C,
}

/// `condition_column` names a column only when the condition narrows it to
/// values; a negation, a column on the right or a nested `OR` reads as
/// `None`, so a "requires a condition on `tenant_id`" check fails closed.
pub fn assert_condition_column_names_only_narrowing<T: TableSource>(
    db: &T,
    shapes: NarrowingShapes<T::Condition>,
) {
    assert_eq!(
        db.condition_column(&shapes.gt),
        Some("invoice_id".to_string())
    );
    for (i, negation) in shapes.negations.iter().enumerate() {
        assert_eq!(db.condition_column(negation), None, "negation #{i}");
    }
    assert_eq!(db.condition_column(&shapes.column_vs_column), None);
    assert_eq!(db.condition_column(&shapes.or_operand), None);
}

pub fn assert_in_values_names_its_column<T: TableSource>(db: &T, value: T::Value) {
    let condition = db.in_values_condition("invoice_id", vec![value]).unwrap();
    assert_eq!(
        db.condition_column(&condition),
        Some("invoice_id".to_string())
    );
}
