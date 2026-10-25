# Diorama tests: one `settle` helper instead of hand-rolled waits

Later work, not part of the CI pipeline PRs.

## Why

The diorama tests wait for background loads in many hand-rolled ways:

- 47 `wait_for_gen` call sites in 19 files, each with the same `subscribe` →
  `borrow_and_update` → act → wait-past-generation steps;
- 31 wait helpers in total, including four local copies of `wait_for_gen` and
  several `eventually` / `wait_until` / `poll_until` variants;
- two helpers named `settle()` (`support/chunk.rs`, `support/aggregate.rs`)
  that are fixed sleeps (250ms, 5× debounce): speed assumptions that can go flaky.

## What

1. Add to `vantage-diorama/tests/support/waiting.rs`:

   ```rust
   /// Run `action`, then wait until the scenery has reacted to it and is no
   /// longer loading. Subscribes first, so a load finishing early isn't missed.
   pub async fn settle<T>(scenery: &Arc<dyn TableScenery>, action: impl Future<Output = T>) -> T
   ```

   Built like `wait_listed`: the generation watch plus `LoadState`, with a
   timeout that bounds a hang (not a speed assertion). Check whether value and
   record sceneries expose the same signals; add sibling helpers only if they do.
2. Replace the `wait_for_gen` dances with `settle(&scenery, <action>)`.
3. Replace the sleep-based `settle()` helpers wherever a real signal exists.
4. Fold wait helpers that are the same thing under different names into
   `support/`; leave ones that wait on something genuinely different.

No public API change: test support only.

## Done when

- `rg 'wait_for_gen|borrow_and_update' vantage-diorama/tests` finds only `support/`.
- No sleep-based `settle()` remains where a generation or `LoadState` signal exists.
- The diorama suite passes three runs in a row with `--all-features`.
- The coverage matrix for vantage-diorama loses nothing beyond its tolerance.
