# vantage-cmd subprocesses have no timeout and unbounded output capture

- **Severity:** medium
- **Category:** omissions
- **Location:** `vantage-cmd/src/exec.rs:66`

`run_command` uses `std::process::Command::output()` with no deadline: a wrapped CLI that hangs (e.g. `gh` waiting on an interactive prompt or a stuck network call with a cleared env) blocks the `spawn_blocking` thread forever, and the table read in Vantage UI never completes. There is also no cap on captured output — `output()` buffers all of stdout/stderr in memory, so a command that streams gigabytes (e.g. a log dump) grows the host process unboundedly before the Rhai script ever sees it.

```rust
let output = cmd.output().map_err(|e| {
    error!(
        "failed to execute command",
        command = command.to_string(),
        detail = e.to_string()
    )
})?;
```

**Recommendation:** Spawn with piped stdio and enforce a configurable timeout (kill the child on expiry) plus a max-output-bytes cap; also set `stdin(Stdio::null())` so child tools cannot block waiting for input.
