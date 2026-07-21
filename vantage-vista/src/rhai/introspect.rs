//! Schema/capability introspection builders backing the `capabilities()`,
//! `columns()`, and `references()` fetch verbs. Each turns a [`Vista`]'s
//! metadata into a plain Rhai value the script can read.

use rhai::{Array, Dynamic, Map as RhaiMap};

use crate::vista::Vista;

/// `{ can_count, can_insert, … }` — every capability flag as a bool.
pub(crate) fn capabilities_map(vista: &Vista) -> RhaiMap {
    let c = vista.capabilities();
    let mut m = RhaiMap::new();
    let mut put = |k: &str, b: bool| {
        m.insert(k.into(), b.into());
    };
    put("can_count", c.can_count);
    put("can_insert", c.can_insert);
    put("can_update", c.can_update);
    put("can_delete", c.can_delete);
    put("can_subscribe", c.can_subscribe);
    put("can_invalidate", c.can_invalidate);
    put("can_order", c.can_order);
    put("can_search", c.can_search);
    put("can_set_page_size", c.can_set_page_size);
    put("can_fetch_page", c.can_fetch_page);
    put("can_fetch_next", c.can_fetch_next);
    put("can_fetch_window", c.can_fetch_window);
    put("can_traverse_to_record", c.can_traverse_to_record);
    put("can_traverse_to_set", c.can_traverse_to_set);
    put("can_build_ref_via_script", c.can_build_ref_via_script);
    put("can_traverse_in_columns", c.can_traverse_in_columns);
    m
}

/// `[{ name, type, flags }, …]` — one entry per column.
pub(crate) fn columns_array(vista: &Vista) -> Array {
    let mut arr = Array::new();
    for name in vista.get_column_names() {
        let mut m = RhaiMap::new();
        m.insert("name".into(), name.to_string().into());
        if let Some(col) = vista.get_column(name) {
            m.insert("type".into(), col.original_type.clone().into());
            let flags: Array = col.flags.iter().map(|f| f.clone().into()).collect();
            m.insert("flags".into(), Dynamic::from_array(flags));
        }
        arr.push(Dynamic::from_map(m));
    }
    arr
}

/// `[{ name, kind, contained }, …]` — referenced and contained relations.
pub(crate) fn references_array(vista: &Vista) -> Array {
    let mut arr = Array::new();
    for (name, kind) in vista.list_references() {
        let mut m = RhaiMap::new();
        m.insert("name".into(), name.into());
        m.insert("kind".into(), format!("{kind:?}").into());
        m.insert("contained".into(), false.into());
        arr.push(Dynamic::from_map(m));
    }
    for (name, kind) in vista.list_contained() {
        let mut m = RhaiMap::new();
        m.insert("name".into(), name.into());
        m.insert("kind".into(), format!("{kind:?}").into());
        m.insert("contained".into(), true.into());
        arr.push(Dynamic::from_map(m));
    }
    arr
}
