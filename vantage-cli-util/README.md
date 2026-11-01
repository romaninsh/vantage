# vantage-cli-util

Helpers for command-line tools built on [Vantage](https://github.com/romaninsh/vantage):
terminal tables, machine-readable output formats, and a generic runner that
drives a `Vista` from command-line tokens.

```toml
[dependencies]
vantage-cli-util = "0.7.0"
```

## Printing tables

`print_table(&table)` lists a `Table` and prints it as a styled terminal
table. `render_records`, `render_records_columns` and `render_records_typed`
do the same for records you already have.

```rust,ignore
vantage_cli_util::print_table(&products).await?;
```

## Output formats

`OutputFormat` is `Table` (default), `Json`, `Ndjson` or `CborDiag`, parsed
from a `--format=` value with `OutputFormat::parse`. `output::render_list`,
`render_record` and `render_scalar` produce the non-table formats as a
`String`. JSON is lossy (large integers become strings, CBOR tags are
dropped). `cbor-diag` is CBOR diagnostic notation (RFC 8949 section 8): lossless and
deterministic, used for golden test fixtures that every Vista driver must
match byte for byte.

## `vista_cli`

`vista_cli::run(&factory, &renderer, &args)` parses positional tokens and
applies them to a Vista. You supply two traits:

- `ModelFactory::for_name(name)` returns a `Vista` and a `Mode` (`List` for
  plural names, `Single` for singular). `for_locator` resolves ids such as
  `user:abc123` or ARNs.
- `Renderer` prints lists, single records and aggregate results.

```rust,ignore
impl ModelFactory for SqliteFactory {
    fn for_name(&self, name: &str) -> Option<(Vista, Mode)> {
        let factory = self.db.vista_factory();
        match name {
            "client" => Some((factory.from_table(Client::sqlite_table(self.db.clone())).ok()?, Mode::Single)),
            "clients" => Some((factory.from_table(Client::sqlite_table(self.db.clone())).ok()?, Mode::List)),
            _ => None,
        }
    }
}

vista_cli::run(&SqliteFactory { db }, &renderer, &args).await?;
```

Tokens include `field=value`, `field:gt=value`, `[0]` (pick a row),
`[+name]` / `[-name]` (sort), `:relation` (follow a relation),
`=col1,col2` (choose columns), `?keyword` (search) and `@count` /
`@sum:field` (aggregates). Some of these are parsed but not wired through
yet (operators other than `eq`, `[N:M]` slices, aggregates); the runner
reports them through `Renderer::note_stub`. The module docs list the full
grammar.

## Used by

`bakery_model3/examples/cli-vista.rs` (one CLI over CSV, SQLite, Postgres,
MongoDB and SurrealDB), `vantage-aws/examples/aws-cli.rs`,
`vantage-cmd/examples/aws-cli.rs` and `vantage-kubernetes/examples/k8s-cli.rs`.
