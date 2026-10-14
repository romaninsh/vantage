//! Spec lookups every driver factory makes while lowering a [`VistaSpec`].

use std::sync::Arc;

use vantage_core::{Result, error};

use super::{ColumnSpec, ReferenceSpec, VistaSpec};
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spec::NoExtras;

    fn spec(yaml: &str) -> VistaSpec<NoExtras, NoExtras, NoExtras> {
        serde_yaml_ng::from_str(yaml).unwrap()
    }

    #[test]
    fn id_column_prefers_explicit_then_flag_then_fallback() {
        let explicit = spec("name: a\nid_column: code\ncolumns: { key: { flags: [id] } }");
        assert_eq!(explicit.resolve_id_column("_id"), "code");

        let flagged = spec("name: a\ncolumns: { name: {}, key: { flags: [id] } }");
        assert_eq!(flagged.resolve_id_column("_id"), "key");

        let bare = spec("name: a\ncolumns: { name: {} }");
        assert_eq!(bare.resolve_id_column("id"), "id");
        assert_eq!(bare.resolve_id_column("_id"), "_id");
    }

    #[test]
    fn foreign_key_prefers_explicit_then_the_relation_name() {
        let spec = spec(
            "name: a\ncolumns: {}\nreferences:\n  \
             owner: { table: users, foreign_key: owner_id }\n  team: { table: teams }",
        );
        assert_eq!(spec.references["owner"].foreign_key("owner"), "owner_id");
        assert_eq!(spec.references["team"].foreign_key("team"), "team");
    }

    #[test]
    fn a_reference_lowers_its_foreign_key_or_its_keys() {
        let spec = spec(
            "name: a\ncolumns: {}\nreferences:\n  \
             owner: { table: users, foreign_key: owner_id }\n  \
             orders: { table: orders, kind: has_many }\n  \
             deploys: { table: deployments, kind: has_many, keys: \
             [{ from: product_id, to: product }, { from: version_id, to: version }] }\n  \
             lines: { table: lines, kind: contains_many }",
        );
        let owner = spec.references["owner"]
            .to_reference("owner", "id")
            .unwrap();
        assert_eq!(
            (owner.kind, owner.target.as_str(), owner.keys.clone()),
            (
                ReferenceKind::HasOne,
                "users",
                vec![JoinKey::to_target_id("owner_id")]
            )
        );
        let orders = spec.references["orders"]
            .to_reference("orders", "code")
            .unwrap();
        assert_eq!(orders.keys, vec![JoinKey::new("code", "orders")]);
        let deploys = spec.references["deploys"]
            .to_reference("deploys", "id")
            .unwrap();
        assert_eq!(
            deploys.keys,
            vec![
                JoinKey::new("product_id", "product"),
                JoinKey::new("version_id", "version")
            ]
        );
        assert!(
            spec.references["lines"]
                .to_reference("lines", "id")
                .is_err()
        );
    }

    #[test]
    fn base_spec_needs_a_resolver_that_knows_it() {
        let none: Option<Arc<dyn SpecResolver<u8>>> = None;
        assert!(resolve_base_spec(none, "b").is_err());

        let resolver: Arc<dyn SpecResolver<u8>> = Arc::new(|name: &str| (name == "b").then_some(7));
        assert!(resolve_base_spec(Some(resolver.clone()), "c").is_err());
        assert_eq!(resolve_base_spec(Some(resolver), "b").unwrap().1, 7);
    }
}
