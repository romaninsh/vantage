//! Schema/capability introspection builders backing the `capabilities()`,
//! `columns()`, and `references()` read verbs. Each turns a [`Vista`]'s
//! metadata into a plain Rhai value the script can read.

use vantage_rhai::rhai::{Array, Dynamic, Map as RhaiMap};

use crate::{Capability, vista::Vista};

/// `{ insert, update, … }`: every capability, by name, as a bool.
pub(crate) fn capabilities_map(vista: &Vista) -> RhaiMap {
    let caps = vista.capabilities();
    Capability::iter()
        .map(|c| (c.as_str().into(), caps.has(c).into()))
        .collect()
}

/// `[{ name, type, flags }, …]` — one entry per column.
pub(crate) fn columns_array(vista: &Vista) -> Array {
    vista
        .columns()
        .values()
        .map(|col| {
            let mut m = RhaiMap::new();
            m.insert("name".into(), col.name.clone().into());
            m.insert("type".into(), col.original_type.clone().into());
            let flags: Array = col.flags().iter().map(|f| f.as_str().into()).collect();
            m.insert("flags".into(), Dynamic::from_array(flags));
            Dynamic::from_map(m)
        })
        .collect()
}

/// `[{ name, kind, contained }, …]` — joined and contained relations.
pub(crate) fn references_array(vista: &Vista) -> Array {
    vista
        .references()
        .values()
        .map(|r| {
            let mut m = RhaiMap::new();
            m.insert("name".into(), r.name.clone().into());
            m.insert("kind".into(), format!("{:?}", r.kind).into());
            m.insert("contained".into(), r.kind.is_contained().into());
            Dynamic::from_map(m)
        })
        .collect()
}
