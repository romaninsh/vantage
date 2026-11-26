//! Spec lookups every driver factory makes while lowering a [`VistaSpec`].

use std::sync::Arc;

use indexmap::IndexMap;
use vantage_core::{Result, error};

use super::{ColumnSpec, ContainedYaml, ReferenceSpec, VistaSpec};
use crate::column::ColumnFlag;
use crate::reference::{JoinKey, Reference, ReferenceKind};

/// Looks up a spec by vista name. Factories hold it as
/// `Arc<dyn SpecResolver<S>>` and hand clones into each reference, so a child
/// table is rebuilt from the current spec at traversal time. Any
/// `Fn(&str) -> Option<S>` closure is a resolver.
pub trait SpecResolver<S>: Send + Sync {
    fn resolve(&self, name: &str) -> Option<S>;
}

impl<S, F: Fn(&str) -> Option<S> + Send + Sync> SpecResolver<S> for F {
    fn resolve(&self, name: &str) -> Option<S> {
        self(name)
    }
}

impl<T, C, R> VistaSpec<T, C, R> {
    /// The id column: `id_column:`, else the first column flagged `id`, else
    /// `default`, the driver's own convention (`"id"`, MongoDB's `"_id"`).
    pub fn resolve_id_column(&self, default: &str) -> String {
        if let Some(id) = &self.id_column {
            return id.clone();
        }
        self.columns
            .iter()
            .find(|(_, col)| col.has_flag(ColumnFlag::Id))
            .map_or_else(|| default.to_string(), |(name, _)| name.clone())
    }
}

impl<T, C: Clone + Default, R> VistaSpec<T, C, R> {
    /// The contained relations the spec declares: its `contained:` entries,
    /// then one `contains_one` per `nested_path` host, named after the host
    /// and declaring each key a nested column reads (`label` at
    /// `name.en_US` makes `name` with an `en_US` column, typed as `label`).
    /// A deeper path keeps its rest as the key (`geo.lat` in `address`);
    /// a one-segment path names no host and makes none.
    ///
    /// Errors when a path has an empty segment, when a nested column is
    /// also `expr:` or `lazy:`, when its host is the column itself, a
    /// computed or nested column, or a `contained:` relation or host.
    pub fn contained_relations(&self) -> Result<IndexMap<String, ContainedYaml<C>>> {
        let mut relations = self.contained.clone();
        for (name, column) in &self.columns {
            let Some(path) = &column.nested_path else {
                continue;
            };
            let refuse =
                |reason: &str| error!(reason.to_string(), column = name, nested_path = path);
            if path.split('.').any(str::is_empty) {
                return Err(refuse("nested_path has an empty segment"));
            }
            if column.expr.is_some() || column.lazy.is_some() {
                return Err(refuse(
                    "nested_path can't be combined with `expr:` or `lazy:`",
                ));
            }
            let Some((host, key)) = path.split_once('.') else {
                continue;
            };
            let computed = self
                .columns
                .get(host)
                .is_some_and(|h| h.expr.is_some() || h.lazy.is_some() || h.nested_path.is_some());
            if host == name || computed {
                return Err(refuse("nested_path's host must be a stored column"));
            }
            let declared = self.contained.contains_key(host)
                || self.contained.values().any(|c| c.host_column == host);
            if declared {
                return Err(refuse(
                    "nested_path's host is already a `contained:` relation or host",
                ));
            }
            let mut key_column = ColumnSpec::new();
            key_column.col_type = column.col_type.clone();
            relations
                .entry(host.to_string())
                .or_insert_with(|| ContainedYaml {
                    host_column: host.to_string(),
                    kind: ReferenceKind::ContainsOne,
                    id_column: None,
                    columns: IndexMap::new(),
                })
                .columns
                .insert(key.to_string(), key_column);
        }
        Ok(relations)
    }
}

impl<C> ColumnSpec<C> {
    pub fn has_flag(&self, flag: impl Into<ColumnFlag>) -> bool {
        self.flags.contains(&flag.into())
    }
}

impl<R> ReferenceSpec<R> {
    /// The `foreign_key:` column, else `relation`, the name the reference is
    /// declared under.
    pub fn foreign_key(&self, relation: &str) -> String {
        self.foreign_key
            .clone()
            .unwrap_or_else(|| relation.to_string())
    }

    /// The join this entry declares under `relation`. Its `keys:` when given
    /// (each pair reads a parent-row column and constrains a target column),
    /// else the single [`foreign_key`](Self::foreign_key): on the parent for
    /// `has_one` (matched to the target's id), on the target for `has_many`
    /// (matched to the parent's `source_id`). A contained kind is refused:
    /// contained relations are declared under `contained:`.
    pub fn to_reference(&self, relation: &str, source_id: &str) -> Result<Reference> {
        let mut keys = self.keys.iter().cloned();
        let first = keys.next();
        let target = self.table.clone();
        let join = match self.kind {
            ReferenceKind::HasOne => Reference::has_one(
                relation,
                target,
                first.unwrap_or_else(|| JoinKey::to_target_id(self.foreign_key(relation))),
            ),
            ReferenceKind::HasMany => Reference::has_many(
                relation,
                target,
                first.unwrap_or_else(|| JoinKey::new(source_id, self.foreign_key(relation))),
            ),
            _ => {
                return Err(error!(
                    "references: entries must be has_one or has_many",
                    relation = relation
                ));
            }
        };
        Ok(keys.fold(join, Reference::with_key))
    }
}

/// Resolve the spec a `base:` vista derives from. Errors when no resolver is
/// attached or the base isn't known to it.
pub fn resolve_base_spec<S>(
    resolver: Option<Arc<dyn SpecResolver<S>>>,
    base: &str,
) -> Result<(Arc<dyn SpecResolver<S>>, S)> {
    let resolver = resolver.ok_or_else(|| {
        error!(
            "vista declares `base:` but no spec resolver is attached to the factory",
            base = base
        )
    })?;
    let spec = resolver
        .resolve(base)
        .ok_or_else(|| error!("base vista not found via resolver", base = base))?;
    Ok((resolver, spec))
}
