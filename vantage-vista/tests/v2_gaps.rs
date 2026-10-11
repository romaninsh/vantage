//! Gap tests: each one asserts a behaviour the 2.0 interfaces guarantee and
//! today's code does not. They stay `#[ignore]`d, with the phase that fixes
//! them in the reason, until that phase lands and they pass.
//!
//! Run them with `--run-ignored only` to see the current failures. The script
//! tests need the `rhai` feature and compile out without it.

mod v2_gaps {
    mod capability_flag_complete;
    #[cfg(feature = "rhai")]
    mod rhai_denied_writes_consistent;
    #[cfg(feature = "rhai")]
    mod rhai_limit_without_page_size;
    #[cfg(feature = "rhai")]
    mod rhai_resolve_self_twice;
    #[cfg(feature = "rhai")]
    mod rhai_support;
}
