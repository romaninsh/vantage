//! A `modify:` script must be able to resolve `self` more than once.
//!
//! Every shell is Clone, so the handle base is a Vista cloned on each
//! resolve. `Wrapped` is a wrapper written with `forward_table_shell!`.

use vantage_dataset::ReadableValueSet;
use vantage_vista::rhai::{block_on, eval_modify_script};
use vantage_vista::{TableShell, Vista, forward_table_shell};

use super::rhai_support::{host, mock, reads};

/// Forwards everything to the inner shell.
#[derive(Clone)]
struct Wrapped {
    inner: Box<dyn TableShell>,
}

forward_table_shell!(Wrapped, inner, {});

#[test]
fn rhai_resolve_self_twice() {
    let host = host(mock(), reads());
    let base = Vista::new("t", mock()).map_shell(|inner| Wrapped { inner });
    let script = r#"if self.count() == 3 { self.where("a", 1) } else { self }"#;
    let vista = eval_modify_script(&host, script, base)
        .unwrap_or_else(|e| panic!("resolving self a second time failed: {e}"));
    let ids: Vec<String> = block_on(vista.list_values())
        .unwrap()
        .unwrap()
        .into_keys()
        .collect();
    assert_eq!(ids, ["r1", "r3"]);
}
