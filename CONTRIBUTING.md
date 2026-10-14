# Contributing

## Public-API snapshots

The public API of these crates is recorded in `api/<crate>.txt`:

- vantage-vista
- vantage-diorama

Any change to that surface updates the snapshot in the same PR. CI regenerates the snapshots and
fails on any difference, so the diff in `api/` is what reviewers read to see the API change.

To regenerate:

```sh
mise run api-snapshot    # or: api/snapshot.sh
```

The header of `api/snapshot.sh` pins the nightly and cargo-public-api versions. The script
installs the nightly through rustup, and if the tool is missing or the wrong version, it prints
the install command. Snapshots are built with `--all-features`. A crate gets a snapshot by being
added to `CRATES` in the script.

## Contract tests and gap tests

Every `TableShell` driver has a contract test (`tests/vista_contract.rs` or similar) that calls
`vantage_vista::contract::check_clone` and `check_advertised`. A driver whose shell can't be
cloned also calls `check_narrowing`. Call `check_advertised_writes` only on a disposable fixture,
since it may store rows. The checks need `vantage-vista` with `features = ["contract"]` under
`[dev-dependencies]`.

A gap test asserts the 2.0 behaviour, so it fails on today's code. Mark it
`#[ignore = "v2 phase N: <what fixes it>"]`. The phase that fixes the gap removes the `ignore`.
Run the gap tests with `cargo nextest run --run-ignored only`.
