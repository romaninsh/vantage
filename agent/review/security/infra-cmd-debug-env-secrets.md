# Cmd/CmdSpec Debug output includes declared env (often credentials)

- **Severity:** medium
- **Category:** security
- **Location:** `vantage-cmd/src/cmd.rs:80`

The declared env of a `Cmd` datasource is exactly where credentials for wrapped CLI tools live (`GH_TOKEN`, `AWS_SECRET_ACCESS_KEY`, etc. — that is the point of `with_env`). The manual `Debug` impl for `Cmd` prints `env` values verbatim, and `CmdSpec` (`cmd.rs:16`) derives `Debug` including its per-table `env`. Any tracing/error path that debug-formats the datasource or spec leaks those secrets into logs.

```rust
impl std::fmt::Debug for Cmd {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Cmd")
            .field("command", &self.command)
            .field("env", &self.env)
            ...
            .field("scripts", &self.scripts) // CmdSpec also derives Debug with env
            .finish()
    }
}
```

**Recommendation:** Print env keys only (or `<redacted>` values) in both `Cmd`'s and `CmdSpec`'s `Debug` output, mirroring `AwsAccount`'s redaction.
