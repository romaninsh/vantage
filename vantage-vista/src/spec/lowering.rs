//! Spec lookups every driver factory makes while lowering a [`VistaSpec`].

use std::sync::Arc;

use vantage_core::{Result, error};

use super::{ColumnSpec, ReferenceSpec, VistaSpec};
use crate::flags;

/// Looks up a spec by vista name. Factories hand clones into each reference
/// so a child table is rebuilt from the current spec at traversal time.
pub type SpecResolver<S> = Arc<dyn Fn(&str) -> Option<S> + Send + Sync>;

impl<T, C, R> VistaSpec<T, C, R> {
    /// The id column: `id_column:`, else the first column flagged `id`, else
    /// `"id"`.
    pub fn resolve_id_column(&self) -> String {
        self.resolve_id_column_or("id")
    }

    /// [`resolve_id_column`](Self::resolve_id_column) with a driver's own
    /// last-resort name (MongoDB uses `_id`).
    pub fn resolve_id_column_or(&self, fallback: &str) -> String {
        if let Some(id) = &self.id_column {
            return id.clone();
        }
        self.columns
            .iter()
            .find(|(_, col)| col.has_flag(flags::ID))
            .map_or_else(|| fallback.to_string(), |(name, _)| name.clone())
    }
}

impl<C> ColumnSpec<C> {
    pub fn has_flag(&self, flag: &str) -> bool {
        self.flags.iter().any(|f| f == flag)
    }
}

impl<R> ReferenceSpec<R> {
    /// The foreign key, defaulting to the relation's own name.
    pub fn foreign_key_or(&self, relation: &str) -> String {
        self.foreign_key
            .clone()
            .unwrap_or_else(|| relation.to_string())
    }
}

/// Resolve the spec a `base:` vista derives from. Errors when no resolver is
/// attached or the base isn't known to it.
pub fn resolve_base_spec<S>(
    resolver: Option<SpecResolver<S>>,
    base: &str,
) -> Result<(SpecResolver<S>, S)> {
    let resolver = resolver.ok_or_else(|| {
        error!(
            "vista declares `base:` but no spec resolver is attached to the factory",
            base = base
        )
    })?;
    let spec =
        resolver(base).ok_or_else(|| error!("base vista not found via resolver", base = base))?;
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
        assert_eq!(explicit.resolve_id_column(), "code");

        let flagged = spec("name: a\ncolumns: { name: {}, key: { flags: [id] } }");
        assert_eq!(flagged.resolve_id_column(), "key");

        let bare = spec("name: a\ncolumns: { name: {} }");
        assert_eq!(bare.resolve_id_column(), "id");
        assert_eq!(bare.resolve_id_column_or("_id"), "_id");
    }

    #[test]
    fn base_spec_needs_a_resolver_that_knows_it() {
        let none: Option<SpecResolver<u8>> = None;
        assert!(resolve_base_spec(none, "b").is_err());

        let resolver: SpecResolver<u8> = Arc::new(|name| (name == "b").then_some(7));
        assert!(resolve_base_spec(Some(resolver.clone()), "c").is_err());
        assert_eq!(resolve_base_spec(Some(resolver), "b").unwrap().1, 7);
    }
}
