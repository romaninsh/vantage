use std::{any::Any, marker::PhantomData, sync::Arc};

use vantage_core::{Entity, Result, error};
use vantage_expressions::{Expressive, SelectSource};

use crate::{
    references::RelatedTable,
    table::Table,
    traits::{column_like::ColumnLike, table_source::TableSource},
};

/// One-to-one relationship reference
///
/// Example: Client has one Bakery (via bakery_id)
/// - foreign_key: "bakery_id" (column on Client table)
/// - get_table: returns Table<SurrealDB, Bakery>
pub struct ReferenceOne<T: TableSource, SourceE: Entity, TargetE: Entity> {
    /// Foreign key column name on the source table
    our_foreign_key: String,
    /// Factory function that creates the target table
    get_table: Arc<dyn Fn() -> Table<T, TargetE> + Send + Sync>,
    _phantom: PhantomData<(T, SourceE, TargetE)>,
}

impl<T: TableSource, SourceE: Entity, TargetE: Entity> ReferenceOne<T, SourceE, TargetE> {
    /// Create a new one-to-one reference
    ///
    /// # Arguments
    /// * `our_foreign_key` - Column name on source table (e.g., "bakery_id")
    /// * `get_table` - Closure that returns the target table
    pub fn new(
        our_foreign_key: impl Into<String>,
        get_table: impl Fn() -> Table<T, TargetE> + Send + Sync + 'static,
    ) -> Self {
        Self {
            our_foreign_key: our_foreign_key.into(),
            get_table: Arc::new(get_table),
            _phantom: PhantomData,
        }
    }
}

impl<T: TableSource, SourceE: Entity, TargetE: Entity> std::fmt::Debug
    for ReferenceOne<T, SourceE, TargetE>
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ReferenceOne")
            .field("our_foreign_key", &self.our_foreign_key)
            .finish()
    }
}

impl<T: TableSource, SourceE: Entity, TargetE: Entity> Clone for ReferenceOne<T, SourceE, TargetE> {
    fn clone(&self) -> Self {
        Self {
            our_foreign_key: self.our_foreign_key.clone(),
            get_table: self.get_table.clone(),
            _phantom: PhantomData,
        }
    }
}

impl<T: TableSource, SourceE: Entity, TargetE: Entity> RelatedTable
    for ReferenceOne<T, SourceE, TargetE>
where
    T::Column: ColumnLike,
    T: SelectSource + Expressive<T::Value>,
    T::Value: Clone + Send + Sync + 'static,
{
    fn get_related_table(&self, source_table: &dyn Any) -> Result<Box<dyn Any>> {
        let source = source_table
            .downcast_ref::<Table<T, SourceE>>()
            .ok_or_else(|| error!("Source table type mismatch in ReferenceOne"))?;
        let mut target = (self.get_table)();

        let target_id = target
            .column("id")
            .ok_or_else(|| error!("Target table must have 'id' column"))?;
        let source_fk = source.column(&self.our_foreign_key).ok_or_else(|| {
            error!(
                "Foreign key not found on source table",
                column = self.our_foreign_key.as_str()
            )
        })?;

        // Build subquery: SELECT fk FROM source WHERE <source conditions>
        let mut subquery = source.select();
        subquery.clear_fields();
        subquery.add_field(source_fk.name());

        let condition = target.data_source().expr(
            format!("{} IN ({})", target_id.name(), "{}"),
            vec![Expressive::nested(subquery.into())],
        );
        target.add_condition(condition);
        Ok(Box::new(target))
    }

    fn get_linked_table(&self, source_table: &dyn Any) -> Result<Box<dyn Any>> {
        let source = source_table
            .downcast_ref::<Table<T, SourceE>>()
            .ok_or_else(|| error!("Source table type mismatch in ReferenceOne"))?;
        let mut target = (self.get_table)();

        let target_id = target
            .column("id")
            .ok_or_else(|| error!("Target table must have 'id' column"))?;
        let source_fk = source.column(&self.our_foreign_key).ok_or_else(|| {
            error!(
                "Foreign key not found on source table",
                column = self.our_foreign_key.as_str()
            )
        })?;

        let condition = target.data_source().expr(
            format!(
                "{}.{} = {}.{}",
                target.table_name(),
                target_id.name(),
                source.table_name(),
                source_fk.name()
            ),
            vec![],
        );
        target.add_condition(condition);
        Ok(Box::new(target))
    }

    fn target_type_name(&self) -> &'static str {
        std::any::type_name::<Table<T, TargetE>>()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mocks::tablesource::MockTableSource;

    #[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
    struct ClientEntity {
        id: i32,
        name: String,
        bakery_id: i32,
    }

    #[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
    struct BakeryEntity {
        id: i32,
        name: String,
    }

    #[test]
    fn test_traversing_from_clienct_to_bakery() {
        // Create a reference using the same pattern as with_one method
        let ds = MockTableSource::new();

        let ref_bakery = ReferenceOne::<MockTableSource, ClientEntity, BakeryEntity>::new(
            "bakery_id",
            move || Table::<MockTableSource, BakeryEntity>::new("bakery", ds.clone()),
        );

        // Test query condition building by using the existing target_table
        let client_table = Table::<MockTableSource, ClientEntity>::new("client", ds.clone());

        // Call get_related_table to create target_table with condition on bakery.id in (client.bakery_id)
        let bakery_any = ref_bakery
            .get_related_table(&client_table as &dyn std::any::Any)
            .unwrap();

        let bakery_table = bakery_any
            .downcast::<Table<MockTableSource, BakeryEntity>>()
            .unwrap();

        // Verify the expected query structure
        assert_eq!(
            bakery_table.select().preview(),
            "select * from bakery where id in (select bakery_id from client)"
        );
    }
}
