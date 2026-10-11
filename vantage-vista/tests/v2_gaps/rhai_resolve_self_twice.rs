//! A `modify:` script over a shell that can't be copied must be able to
//! resolve `self` more than once.
//!
//! Today `Handle::over` keeps the Vista in a take-once slot: without
//! `clone_shell`, the first resolve (`self.count()`) moves the Vista out and
//! the script's result can't be resolved. In 2.0 the base is an `Arc<Vista>`
//! cloned on each resolve.
//!
//! A shell that can't be cloned can't exist in 2.0. Phase 1 turns
//! `Uncopyable` into a `#[derive(Clone)]` wrapper; the assertion stays. Once
//! every shell is cloneable this may already pass after phase 1, before the
//! base itself changes.

use vantage_dataset::ReadableValueSet;
use vantage_vista::rhai::block_on;
use vantage_vista::{TableShell, Terminals, Vista, eval_modify_script, forward_table_shell};

use super::rhai_support::{host, mock};

/// Forwards everything to the inner shell except `clone_shell`, which stays
/// `None` the way several real drivers leave it today.
struct Uncopyable {
    inner: Box<dyn TableShell>,
}

forward_table_shell!(Uncopyable, inner, {
    fn clone_shell(&self) -> Option<Box<dyn TableShell>> {
        None
    }
});

#[test]
#[ignore = "v2 phase 2: the handle base becomes an Arc<Vista> cloned on each resolve"]
fn rhai_resolve_self_twice() {
    let host = host(mock(), Terminals::Read { limit: None });
    let base = Vista::new(
        "t",
        Box::new(Uncopyable {
            inner: Box::new(mock()),
        }),
    );
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
