//! Gap tests: each one asserts a behaviour the 2.0 interfaces guarantee and
//! today's code does not. They stay `#[ignore]`d, with the phase that fixes
//! them in the reason, until that phase lands and they pass.
//!
//! Run them with `--run-ignored only` to see the current failures.

mod support;

mod v2_gaps {
    mod dio_base_conditions_reach_every_path;
    mod dio_with_does_not_mutate_holders;
    mod fixtures;
    // Awaits the write worker's exit through `take_write_worker_handle`.
    #[cfg(feature = "test-util")]
    mod id_column_not_hardcoded;
    mod paged_scenery_conditions;
    mod paged_search_on_uncloneable_master;
    mod shared_scenery_cross_talk;
}
