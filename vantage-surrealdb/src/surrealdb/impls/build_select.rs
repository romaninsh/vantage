use vantage_table::table::Table;
use vantage_types::Entity;

use crate::select::SurrealSelect;
use crate::surrealdb::SurrealDB;
use crate::types::AnySurrealType;

/// Build a `SurrealSelect` from a `Table<SurrealDB, E>`'s current state
/// (source, conditions, ordering).
pub fn build_select<E>(table: &Table<SurrealDB, E>) -> SurrealSelect
where
    E: Entity<AnySurrealType>,
{
    use crate::identifier::Identifier;
    use crate::select::select_field::SelectField;
    use crate::select::target::Target;
    use vantage_table::sorting::SortDirection;

    let mut select = SurrealSelect::new();

    // Source
    select.from = vec![Target::new(Identifier::new(table.table_name()))];

    // Columns → fields
    for col in table.columns().values() {
        match col.alias() {
            Some(alias) => {
                let field =
                    SelectField::new(Identifier::new(col.name())).with_alias(alias.to_string());
                select.fields.push(field);
            }
            None => {
                select
                    .fields
                    .push(SelectField::new(Identifier::new(col.name())));
            }
        }
    }

    // Conditions
    for condition in table.conditions() {
        select.where_conditions.push(condition.clone());
    }

    // Ordering
    for (expr, dir) in table.orders() {
        let ascending = matches!(dir, SortDirection::Ascending);
        select.order_by.push((expr.clone(), ascending));
    }

    // TODO: pagination — needs access without TableLike's 'static bound

    select
}
