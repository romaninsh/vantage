//! HasMany — one-to-many relationship where the foreign key is on the target table.
//!
//! Example: Bakery has many Clients (via client.bakery_id)

use std::fmt::Display;
use std::{any::Any, marker::PhantomData, sync::Arc};

use vantage_core::{Result, error};
use vantage_types::{EmptyEntity, Entity, Record};
use vantage_vista::{JoinKey, Reference as VistaReference};

use crate::{references::Reference, table::Table, traits::table_source::TableSource};

pub struct HasMany<T: TableSource, SourceE: Entity<T::Value>, TargetE: Entity<T::Value>> {
    /// Foreign key column on the target table (e.g. "bakery_id" on client)
    target_foreign_key: String,
    /// Source column the foreign key holds; `None` is the source's id.
    source_column: Option<String>,
    /// Further keys the target is narrowed by, each fully named.
    more_keys: Vec<JoinKey>,
    /// Factory: given a data source, produce the target table
    build_target: Arc<dyn Fn(T) -> Result<Table<T, TargetE>> + Send + Sync>,
    /// The target's metadata name (see `target_table_name`).
    target_hint: Option<String>,
    _phantom: PhantomData<SourceE>,
}

impl<T: TableSource, SourceE: Entity<T::Value>, TargetE: Entity<T::Value>>
    HasMany<T, SourceE, TargetE>
{
    pub fn new(
        target_foreign_key: impl Into<String>,
        build_target: impl Fn(T) -> Table<T, TargetE> + Send + Sync + 'static,
    ) -> Self {
        Self {
            target_foreign_key: target_foreign_key.into(),
            build_target: Arc::new(move |source| Ok(build_target(source))),
            target_hint: None,
            source_column: None,
            more_keys: Vec::new(),
            _phantom: PhantomData,
        }
    }

    /// A relation whose target build can fail; traversal returns the error.
    /// `target` names the target in metadata; the build isn't run for it.
    pub fn try_new(
        target_foreign_key: impl Into<String>,
        target: impl Into<String>,
        build_target: impl Fn(T) -> Result<Table<T, TargetE>> + Send + Sync + 'static,
    ) -> Self {
        Self {
            target_foreign_key: target_foreign_key.into(),
            build_target: Arc::new(build_target),
            target_hint: Some(target.into()),
            source_column: None,
            more_keys: Vec::new(),
            _phantom: PhantomData,
        }
    }

    /// Join by `keys` instead: the first key's `from` source column is what
    /// the foreign key holds, and every further key narrows the target too.
    /// A key without a target column is an error.
    pub fn with_keys(mut self, keys: &[JoinKey]) -> Result<Self> {
        let mut keys = keys.iter();
        let first = keys
            .next()
            .ok_or_else(|| error!("a has_many join needs at least one key"))?;
        let to = |key: &JoinKey| {
            key.to.clone().ok_or_else(|| {
                error!(
                    "a has_many join key needs a target column",
                    from = key.from.as_str()
                )
            })
        };
        self.target_foreign_key = to(first)?;
        self.source_column = Some(first.from.clone());
        self.more_keys = keys
            .map(|key| Ok(JoinKey::new(&key.from, to(key)?)))
            .collect::<Result<_>>()?;
        Ok(self)
    }
}

impl<T: TableSource, SourceE: Entity<T::Value>, TargetE: Entity<T::Value>> Clone
    for HasMany<T, SourceE, TargetE>
{
    fn clone(&self) -> Self {
        Self {
            target_foreign_key: self.target_foreign_key.clone(),
            build_target: self.build_target.clone(),
            target_hint: self.target_hint.clone(),
            source_column: self.source_column.clone(),
            more_keys: self.more_keys.clone(),
            _phantom: PhantomData,
        }
    }
}

impl<T: TableSource, SourceE: Entity<T::Value>, TargetE: Entity<T::Value>> std::fmt::Debug
    for HasMany<T, SourceE, TargetE>
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HasMany")
            .field("target_foreign_key", &self.target_foreign_key)
            .finish()
    }
}

impl<
    T: TableSource + 'static,
    SourceE: Entity<T::Value> + 'static,
    TargetE: Entity<T::Value> + 'static,
> Reference for HasMany<T, SourceE, TargetE>
where
    T::Value: Into<ciborium::Value> + From<ciborium::Value>,
    T::Id: Display + From<String>,
{
    fn columns(&self, source_id: &str, _target_id: &str) -> (String, String) {
        let source = self.source_column.as_deref().unwrap_or(source_id);
        (source.to_string(), self.target_foreign_key.clone())
    }

    fn vista_reference(&self, name: &str, target: String, source_id: &str) -> VistaReference {
        let (source, foreign_key) = self.columns(source_id, "");
        let mut reference =
            VistaReference::has_many(name, target, JoinKey::new(source, foreign_key));
        reference.keys.extend(self.more_keys.iter().cloned());
        reference
    }

    fn build_target(&self, data_source: &dyn Any) -> Result<Box<dyn Any>> {
        let ds = data_source
            .downcast_ref::<T>()
            .ok_or_else(|| error!("data source type mismatch in HasMany::build_target"))?;
        Ok(Box::new((self.build_target)(ds.clone())?))
    }

    fn build_target_erased(&self, data_source: &dyn Any) -> Result<Box<dyn Any>> {
        let ds = data_source
            .downcast_ref::<T>()
            .ok_or_else(|| error!("data source type mismatch in HasMany::build_target_erased"))?;
        Ok(Box::new(
            (self.build_target)(ds.clone())?.into_entity::<EmptyEntity>(),
        ))
    }

    fn resolve_from_row(
        &self,
        data_source: &dyn Any,
        source_id_field: &str,
        source_row: &dyn Any,
    ) -> Result<Box<dyn Any>> {
        let ds = data_source
            .downcast_ref::<T>()
            .ok_or_else(|| error!("data source type mismatch in HasMany::resolve_from_row"))?;
        let row = source_row
            .downcast_ref::<Record<T::Value>>()
            .ok_or_else(|| error!("source row type mismatch in HasMany::resolve_from_row"))?;

        let mut target = (self.build_target)(ds.clone())?;
        let (src_col, tgt_col) = self.columns(source_id_field, "");
        let keys = std::iter::once(JoinKey::new(src_col, tgt_col)).chain(self.more_keys.clone());
        for key in keys {
            let join_value = row.get(&key.from).cloned().ok_or_else(|| {
                error!("source row missing join field", field = key.from.as_str())
            })?;
            // Rows arriving from a script/JSON surface carry ids in string form.
            let join_value = ds.coerce_reference_value(join_value);
            let to = key.to.unwrap_or_default();
            let condition = ds.eq_value_condition(&to, join_value.clone())?;
            target.add_condition(condition);
            target.add_invariant(to, join_value);
        }

        Ok(Box::new(target.into_entity::<EmptyEntity>()))
    }

    /// The `try_new` target name when set, without building: a metadata
    /// snapshot mustn't run the target's build (and an injector with it).
    fn target_table_name(&self, data_source: &dyn Any) -> String {
        if let Some(hint) = &self.target_hint {
            return hint.clone();
        }
        let ds = data_source
            .downcast_ref::<T>()
            .expect("data source type mismatch in HasMany::target_table_name");
        (self.build_target)(ds.clone())
            .map(|target| target.table_name().to_string())
            .unwrap_or_default()
    }
}
