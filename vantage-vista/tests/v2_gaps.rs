//! Behaviours the `Vista` interfaces guarantee across capabilities and
//! scripts. The script tests need the `rhai` and `mocks` features and
//! compile out without them.

mod v2_gaps {
    mod capability_flag_complete;
    #[cfg(all(feature = "rhai", feature = "mocks"))]
    mod rhai_denied_writes_consistent;
    #[cfg(all(feature = "rhai", feature = "mocks"))]
    mod rhai_limit_without_page_size;
    #[cfg(all(feature = "rhai", feature = "mocks"))]
    mod rhai_resolve_self_twice;
    #[cfg(all(feature = "rhai", feature = "mocks"))]
    mod rhai_support;
}
