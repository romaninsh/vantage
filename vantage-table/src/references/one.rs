//! HasOne — one-to-one relationship where the foreign key is on the source table.
//!
//! Example: Client has one Bakery (via client.bakery_id)

use std::fmt::Display;
use std::{any::Any, marker::PhantomData, sync::Arc};

use vantage_core::Result;
use vantage_types::Entity;

use crate::{
    any::AnyTable,
    references::Reference,
    table::Table,
    traits::{column_like::ColumnLike, table_source::TableSource},
};

pub struct HasOne<T: TableSource, SourceE: Entity<T::Value>, TargetE: Entity<T::Value>> {
    /// Foreign key column on the source table (e.g. "bakery_id")
    foreign_key: String,
    /// Factory: given a data source, produce the target table
    build_target: Arc<dyn Fn(T) -> Table<T, TargetE> + Send + Sync>,
    _phantom: PhantomData<SourceE>,
}

impl<T: TableSource, SourceE: Entity<T::Value>, TargetE: Entity<T::Value>>
    HasOne<T, SourceE, TargetE>
{
    pub fn new(
        foreign_key: impl Into<String>,
        build_target: impl Fn(T) -> Table<T, TargetE> + Send + Sync + 'static,
    ) -> Self {
        Self {
            foreign_key: foreign_key.into(),
            build_target: Arc::new(build_target),
            _phantom: PhantomData,
        }
    }
}

impl<T: TableSource, SourceE: Entity<T::Value>, TargetE: Entity<T::Value>> Clone
    for HasOne<T, SourceE, TargetE>
{
    fn clone(&self) -> Self {
        Self {
            foreign_key: self.foreign_key.clone(),
            build_target: self.build_target.clone(),
            _phantom: PhantomData,
        }
    }
}

impl<T: TableSource, SourceE: Entity<T::Value>, TargetE: Entity<T::Value>> std::fmt::Debug
    for HasOne<T, SourceE, TargetE>
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HasOne")
            .field("foreign_key", &self.foreign_key)
            .finish()
    }
}

impl<
    T: TableSource + 'static,
    SourceE: Entity<T::Value> + 'static,
    TargetE: Entity<T::Value> + 'static,
> Reference for HasOne<T, SourceE, TargetE>
where
    T::Value: Into<ciborium::Value> + From<ciborium::Value>,
    T::Id: Display + From<String>,
{
    fn columns(&self, _source_id: &str, target_id: &str) -> (String, String) {
        (self.foreign_key.clone(), target_id.to_string())
    }

    fn build_target(&self, data_source: &dyn Any) -> Box<dyn Any> {
        let ds = data_source
            .downcast_ref::<T>()
            .expect("data source type mismatch in HasOne::build_target");
        Box::new((self.build_target)(ds.clone()))
    }

    fn resolve_as_any(&self, source_table: &dyn Any) -> Result<AnyTable> {
        let source = source_table
            .downcast_ref::<Table<T, SourceE>>()
            .ok_or_else(|| {
                vantage_core::error!("Source table type mismatch in HasOne::resolve_as_any")
            })?;

        let source_id = source
            .id_field()
            .map(|c| c.name().to_string())
            .unwrap_or_else(|| "id".to_string());

        let mut target = (self.build_target)(source.data_source().clone());

        let target_id = target
            .id_field()
            .map(|c| c.name().to_string())
            .unwrap_or_else(|| "id".to_string());

        let (src_col, tgt_col) = self.columns(&source_id, &target_id);
        let condition = source
            .data_source()
            .related_in_condition(&tgt_col, source, &src_col);
        target.add_condition(condition);

        Ok(AnyTable::from_table(target))
    }

    fn target_type_name(&self) -> &'static str {
        std::any::type_name::<Table<T, TargetE>>()
    }
}
