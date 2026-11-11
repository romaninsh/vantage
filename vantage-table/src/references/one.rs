//! HasOne — one-to-one relationship where the foreign key is on the source table.
//!
//! Example: Client has one Bakery (via client.bakery_id)

use std::fmt::Display;
use std::sync::OnceLock;
use std::{any::Any, marker::PhantomData, sync::Arc};

use vantage_core::{Result, error};
use vantage_types::{EmptyEntity, Entity, Record};
use vantage_vista::{JoinKey, Reference as VistaReference};

use crate::{references::Reference, table::Table, traits::table_source::TableSource};

pub struct HasOne<T: TableSource, SourceE: Entity<T::Value>, TargetE: Entity<T::Value>> {
    /// Foreign key column on the source table (e.g. "bakery_id")
    foreign_key: String,
    /// Target column the foreign key matches; `None` is the target's id.
    target_column: Option<String>,
    /// Further keys the target is narrowed by.
    more_keys: Vec<JoinKey>,
    /// Factory: given a data source, produce the target table
    build_target: Arc<dyn Fn(T) -> Result<Table<T, TargetE>> + Send + Sync>,
    /// The target's metadata name (see `target_table_name`).
    target_hint: Option<String>,
    /// The built target's stored table name, once asked (see
    /// `has_one_target`); shared by clones.
    stored_target: Arc<OnceLock<String>>,
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
            target_column: None,
            more_keys: Vec::new(),
            build_target: Arc::new(move |source| Ok(build_target(source))),
            target_hint: None,
            stored_target: Arc::default(),
            _phantom: PhantomData,
        }
    }

    /// A relation whose target build can fail; traversal returns the error.
    /// `target` names the target in metadata; the build isn't run for it.
    pub fn try_new(
        foreign_key: impl Into<String>,
        target: impl Into<String>,
        build_target: impl Fn(T) -> Result<Table<T, TargetE>> + Send + Sync + 'static,
    ) -> Self {
        Self {
            foreign_key: foreign_key.into(),
            target_column: None,
            more_keys: Vec::new(),
            build_target: Arc::new(build_target),
            target_hint: Some(target.into()),
            stored_target: Arc::default(),
            _phantom: PhantomData,
        }
    }

    /// Join by `keys` instead: the first key's `from` is the foreign key and
    /// its `to` the target column it matches (the id when `None`); every
    /// further key narrows the target too.
    pub fn with_keys(mut self, keys: &[JoinKey]) -> Result<Self> {
        let (first, more) = keys
            .split_first()
            .ok_or_else(|| error!("a has_one join needs at least one key"))?;
        self.foreign_key = first.from.clone();
        self.target_column = first.to.clone();
        self.more_keys = more.to_vec();
        Ok(self)
    }

    /// Joined on the target's id by a single key.
    fn joins_target_id(&self) -> bool {
        self.target_column.is_none() && self.more_keys.is_empty()
    }
}

impl<T: TableSource, SourceE: Entity<T::Value>, TargetE: Entity<T::Value>> Clone
    for HasOne<T, SourceE, TargetE>
{
    fn clone(&self) -> Self {
        Self {
            foreign_key: self.foreign_key.clone(),
            target_column: self.target_column.clone(),
            more_keys: self.more_keys.clone(),
            build_target: self.build_target.clone(),
            target_hint: self.target_hint.clone(),
            stored_target: self.stored_target.clone(),
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
        let target = self.target_column.as_deref().unwrap_or(target_id);
        (self.foreign_key.clone(), target.to_string())
    }

    fn vista_reference(&self, name: &str, target: String, _source_id: &str) -> VistaReference {
        let first = match &self.target_column {
            Some(column) => JoinKey::new(&self.foreign_key, column),
            None => JoinKey::to_target_id(&self.foreign_key),
        };
        let mut reference = VistaReference::has_one(name, target, first);
        reference.keys.extend(self.more_keys.iter().cloned());
        reference
    }

    fn build_target(&self, data_source: &dyn Any) -> Result<Box<dyn Any>> {
        let ds = data_source
            .downcast_ref::<T>()
            .ok_or_else(|| error!("data source type mismatch in HasOne::build_target"))?;
        Ok(Box::new((self.build_target)(ds.clone())?))
    }

    fn build_target_erased(&self, data_source: &dyn Any) -> Result<Box<dyn Any>> {
        let ds = data_source
            .downcast_ref::<T>()
            .ok_or_else(|| error!("data source type mismatch in HasOne::build_target_erased"))?;
        Ok(Box::new(
            (self.build_target)(ds.clone())?.into_entity::<EmptyEntity>(),
        ))
    }

    fn resolve_from_row(
        &self,
        data_source: &dyn Any,
        _source_id_field: &str,
        source_row: &dyn Any,
    ) -> Result<Box<dyn Any>> {
        let ds = data_source
            .downcast_ref::<T>()
            .ok_or_else(|| error!("data source type mismatch in HasOne::resolve_from_row"))?;
        let row = source_row
            .downcast_ref::<Record<T::Value>>()
            .ok_or_else(|| error!("source row type mismatch in HasOne::resolve_from_row"))?;

        let mut target = (self.build_target)(ds.clone())?;
        let target_id = target.id_field_name();

        let (src_col, tgt_col) = self.columns("", &target_id);
        let keys = std::iter::once(JoinKey::new(src_col, tgt_col)).chain(self.more_keys.clone());
        for key in keys {
            let join_value = row.get(&key.from).cloned().ok_or_else(|| {
                error!(
                    "source row missing foreign-key field",
                    field = key.from.as_str()
                )
            })?;
            // Rows arriving from a script/JSON surface carry ids in string form.
            let join_value = ds.coerce_reference_value(join_value);
            let to = key.to.unwrap_or_else(|| target_id.clone());
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
            .expect("data source type mismatch in HasOne::target_table_name");
        (self.build_target)(ds.clone())
            .map(|target| target.table_name().to_string())
            .unwrap_or_default()
    }

    /// The built target's table name, which a spec's driver block may set
    /// apart from the spec name; the metadata name when the build fails.
    /// Built once per relation, and [`bare`](crate::table::inject::bare): only
    /// the name is read.
    fn has_one_target(&self, column: &str, data_source: &dyn Any) -> Option<String> {
        // A join on another target column doesn't hold the target's links.
        if column != self.foreign_key || !self.joins_target_id() {
            return None;
        }
        let name = self.stored_target.get_or_init(|| {
            data_source
                .downcast_ref::<T>()
                .and_then(|ds| crate::table::inject::bare(|| (self.build_target)(ds.clone())).ok())
                .map(|target| target.table_name().to_string())
                .or_else(|| self.target_hint.clone())
                .unwrap_or_default()
        });
        (!name.is_empty()).then(|| name.clone())
    }
}
