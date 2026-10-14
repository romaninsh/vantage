//! Gap tests: each one asserts a behaviour the 2.0 interfaces guarantee and
//! 1.x did not. A test stays `#[ignore]`d, with the phase that fixes it in
//! the reason, until that phase lands and it passes.
//!
//! Run them with `--run-ignored only` to see the current failures. The script
//! tests need the `rhai` and `mocks` features and compile out without them.

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
