# vantage-cmd

A [Vantage](https://github.com/romaninsh/vantage) persistence backend that
gets its data by **running a local command** (the `aws` CLI, `kubectl`,
`gh`, `terraform`, …) and shaping it with a [Rhai](https://rhai.rs) script.

## Security model

- The **command is locked** on the [`Cmd`] datasource. A script can build
  arbitrary *arguments* but can never change which binary runs.
- The child process gets a **cleared environment** plus only the variables
  declared on the datasource / table (and `PATH`/`HOME` so the binary is
  locatable — toggle with `Cmd::with_pass_path(false)`).
- The **argv and the output parsing live in a Rhai script**. On read, the
  script runs with the table's `conditions`, `columns`, `limit`, `offset`
  and `id_column` in scope, calls the registered `run(args)` callback to
  execute the locked command, then parses the captured output into rows.

## Rhai surface

Variables in scope: `conditions` (array of `#{ field, op, value }`),
`columns`, `limit`, `offset`, `id_column`.

Registered functions:

- `run(args)` → `#{ stdout, stderr, exit_code }` — execute the locked command.
- `parse_json(string)` → value — parse JSON (e.g. `aws ... --output json`).
- `parse_jsonl(string)` → array — parse newline-delimited JSON.

The script returns an **array of row objects**; the row's `id_column` field
becomes the record id.

```rhai
let args = ["logs", "describe-log-groups", "--output", "json"];
for c in conditions {
    if c.field == "logGroupNamePrefix" {
        args += ["--log-group-name-prefix", c.value];
    }
}
let out = run(args);
if out.exit_code != 0 { throw out.stderr; }
parse_json(out.stdout).logGroups
```

## Example: an `aws` CLI

`examples/aws-cli.rs` wires the `aws` CLI to the Vista-driven CLI runner,
with YAML-defined models under `vistas/`:

```bash
cargo run --example aws-cli -- log.groups
cargo run --example aws-cli -- iam.users
cargo run --example aws-cli -- log.group <name> :streams
cargo run --example aws-cli -- lambda.function <name> :versions
cargo run --example aws-cli -- s3.bucket <name> :objects
```

### Models and relations

| Model | Command | Relations (`:name`) |
|-------|---------|---------------------|
| `iam.users` | `iam list-users` | `:groups`, `:access_keys`, `:policies` |
| `iam.groups` | `iam list-groups` | — |
| `iam.roles` | `iam list-roles` | — |
| `iam.policies` | `iam list-policies --scope Local` | — |
| `log.groups` | `logs describe-log-groups` | `:streams`, `:events` |
| `ecs.clusters` | `ecs list-clusters` | `:services`, `:tasks` |
| `ecs.task_definitions` | `ecs list-task-definitions` | — |
| `s3.buckets` | `s3api list-buckets` | `:objects` |
| `lambda.functions` | `lambda list-functions` | `:aliases`, `:versions` |

Relations are declared in each vista's `references:` block (target model +
`foreign_key`); traversal pins the child to the parent row's key (e.g.
`log.group X :streams` runs `logs describe-log-streams --log-group-name X`).

Add a model by dropping a `*.yaml` in `vistas/` and registering it in
`CmdModelFactory` (`yaml_for` / `known_names` / `is_singular`).

Status: incubating.
