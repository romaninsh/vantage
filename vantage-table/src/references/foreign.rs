//! HasForeign — cross-persistence reference with user-provided resolution.
//!
//! The user supplies a sync closure that receives the source table and returns
//! an AnyTable with deferred conditions attached. The actual async work
//! (fetching IDs, querying the foreign backend) happens lazily at query time
//! via DeferredFn inside the target's condition system.

use std::{any::Any, marker::PhantomData, sync::Arc};

use vantage_core::Result;
use vantage_types::Entity;

use crate::{
    any::AnyTable, references::Reference, table::Table, traits::table_source::TableSource,
};

type ResolveFn<T, E> = dyn Fn(&Table<T, E>) -> Result<AnyTable> + Send + Sync;

pub struct HasForeign<T: TableSource, SourceE: Entity<T::Value>> {
    /// Sync closure: receives source table, returns AnyTable with deferred conditions
    resolve: Arc<ResolveFn<T, SourceE>>,
    /// Type name for error messages
    target_type: &'static str,
    _phantom: PhantomData<(T, SourceE)>,
}

impl<T: TableSource + 'static, SourceE: Entity<T::Value> + 'static> HasForeign<T, SourceE> {
    pub fn new(
        target_type: &'static str,
        resolve: impl Fn(&Table<T, SourceE>) -> Result<AnyTable> + Send + Sync + 'static,
    ) -> Self {
        Self {
            resolve: Arc::new(resolve),
            target_type,
            _phantom: PhantomData,
        }
    }
}

impl<T: TableSource, SourceE: Entity<T::Value>> Clone for HasForeign<T, SourceE> {
    fn clone(&self) -> Self {
        Self {
            resolve: self.resolve.clone(),
            target_type: self.target_type,
            _phantom: PhantomData,
        }
    }
}

impl<T: TableSource, SourceE: Entity<T::Value>> std::fmt::Debug for HasForeign<T, SourceE> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HasForeign")
            .field("target_type", &self.target_type)
            .finish()
    }
}

impl<T: TableSource + 'static, SourceE: Entity<T::Value> + 'static> Reference
    for HasForeign<T, SourceE>
{
    fn columns(&self, _source_id: &str, _target_id: &str) -> (String, String) {
        unreachable!("columns() should not be called on foreign references")
    }

    fn build_target(&self, _data_source: &dyn Any) -> Box<dyn Any> {
        unreachable!("build_target() should not be called on foreign references")
    }

    fn is_foreign(&self) -> bool {
        true
    }

    fn resolve_as_any(&self, source_table: &dyn Any) -> Result<AnyTable> {
        let source = source_table
            .downcast_ref::<Table<T, SourceE>>()
            .ok_or_else(|| {
                vantage_core::error!("Source table type mismatch in HasForeign::resolve_as_any")
            })?;
        (self.resolve)(source)
    }

    fn target_type_name(&self) -> &'static str {
        self.target_type
    }
}
